//! Lua 5.3 binary chunks in two dialects: stock (`luac` 5.3), and the client's, whose header
//! leaves out the size_t size byte while the dump still writes 8-byte size_t string lengths
//! (spike-0 6.15; docs/re/m1-script-format.md §3).
use crate::{Error, Result};

/// The two chunk dialects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialect {
    Stock53,
    Client,
}

const SIGNATURE: &[u8] = b"\x1bLua";
const LUAC_DATA: &[u8] = b"\x19\x93\r\n\x1a\n";
const LUAC_INT: i64 = 0x5678;
const LUAC_NUM: f64 = 370.5;
/// Deeper nesting is refused: real chunks are shallow, and the recursion must stay bounded.
pub const MAX_DEPTH: usize = 200;

impl Dialect {
    fn sizes(self) -> &'static [u8] {
        match self {
            // int, size_t, Instruction, lua_Integer, lua_Number
            Self::Stock53 => &[4, 8, 4, 8, 8],
            // the same without size_t
            Self::Client => &[4, 4, 8, 8],
        }
    }

    /// The dialect whose header `bytes` starts with, if either.
    pub fn detect(bytes: &[u8]) -> Option<Self> {
        [Self::Stock53, Self::Client]
            .into_iter()
            .find(|d| bytes.starts_with(&header(*d)))
    }
}

/// A dialect's exact header bytes.
pub fn header(d: Dialect) -> Vec<u8> {
    let mut h = Vec::with_capacity(33);
    h.extend_from_slice(SIGNATURE);
    h.extend_from_slice(&[0x53, 0]); // version 5.3, official format
    h.extend_from_slice(LUAC_DATA);
    h.extend_from_slice(d.sizes());
    h.extend_from_slice(&LUAC_INT.to_le_bytes());
    h.extend_from_slice(&LUAC_NUM.to_le_bytes());
    h
}

/// A parsed chunk: the main function and its upvalue count.
#[derive(Debug, Clone, PartialEq)]
pub struct Chunk {
    pub upvalue_count: u8,
    pub main: Function,
}

/// One function prototype, in dump order.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Function {
    pub source: Option<Vec<u8>>,
    pub line_defined: i32,
    pub last_line_defined: i32,
    pub num_params: u8,
    pub is_vararg: u8,
    pub max_stack_size: u8,
    pub code: Vec<u32>,
    pub constants: Vec<Constant>,
    pub upvalues: Vec<Upvalue>,
    pub protos: Vec<Function>,
    pub line_info: Vec<i32>,
    pub loc_vars: Vec<LocVar>,
    pub upvalue_names: Vec<Option<Vec<u8>>>,
}

/// A constant, with Lua 5.3's type tags: 0 nil, 1 boolean, 3 float, 19 integer, 4 short string,
/// 20 long string.
#[derive(Debug, Clone, PartialEq)]
pub enum Constant {
    Nil,
    Boolean(bool),
    Float(f64),
    Integer(i64),
    ShortString(Vec<u8>),
    LongString(Vec<u8>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Upvalue {
    pub in_stack: u8,
    pub index: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocVar {
    pub name: Option<Vec<u8>>,
    pub start_pc: i32,
    pub end_pc: i32,
}

/// Parses a whole chunk of `dialect`, refusing anything left over after the main function.
pub fn parse(bytes: &[u8], dialect: Dialect) -> Result<Chunk> {
    let h = header(dialect);
    if !bytes.starts_with(&h) {
        return Err(Error::Header(dialect));
    }
    let mut r = Reader {
        b: bytes,
        p: h.len(),
    };
    let upvalue_count = r.u8()?;
    let main = r.function(0)?;
    if r.p != bytes.len() {
        return Err(Error::Trailing {
            offset: r.p,
            extra: bytes.len() - r.p,
        });
    }
    Ok(Chunk {
        upvalue_count,
        main,
    })
}

/// Writes `chunk` in `dialect`. For every chunk `parse` accepts, `write(&parse(b, d)?, d) == b`.
pub fn write(chunk: &Chunk, dialect: Dialect) -> Vec<u8> {
    let mut w = header(dialect);
    w.push(chunk.upvalue_count);
    write_function(&mut w, &chunk.main);
    w
}

struct Reader<'a> {
    b: &'a [u8],
    p: usize,
}

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8]> {
        let end = self
            .p
            .checked_add(n)
            .filter(|&e| e <= self.b.len())
            .ok_or(Error::Truncated {
                offset: self.p,
                needed: n,
            })?;
        let s = &self.b[self.p..end];
        self.p = end;
        Ok(s)
    }

    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }

    fn i32(&mut self) -> Result<i32> {
        Ok(i32::from_le_bytes(
            self.take(4)?.try_into().expect("4 bytes"),
        ))
    }

    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(
            self.take(4)?.try_into().expect("4 bytes"),
        ))
    }

    fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_le_bytes(
            self.take(8)?.try_into().expect("8 bytes"),
        ))
    }

    /// A count followed by items of at least `min_item` bytes each. Refuses counts the rest of
    /// the input can't hold, so a corrupt count never causes a huge allocation.
    fn count(&mut self, min_item: usize) -> Result<usize> {
        let at = self.p;
        let n = self.i32()?;
        let bad = Error::BadCount {
            offset: at,
            count: i64::from(n),
        };
        let n = usize::try_from(n).map_err(|_| bad.clone())?;
        if n.saturating_mul(min_item) > self.b.len() - self.p {
            return Err(bad);
        }
        Ok(n)
    }

    /// A string: one size byte (length + 1), or 0xFF and an 8-byte size_t; size 0 is NULL.
    fn string(&mut self) -> Result<Option<Vec<u8>>> {
        let at = self.p;
        let mut size = u64::from(self.u8()?);
        if size == 0xFF {
            size = self.u64()?;
        }
        if size == 0 {
            return Ok(None);
        }
        let len = usize::try_from(size - 1).map_err(|_| Error::BadCount {
            offset: at,
            count: size as i64,
        })?;
        Ok(Some(self.take(len)?.to_vec()))
    }

    fn function(&mut self, depth: usize) -> Result<Function> {
        if depth > MAX_DEPTH {
            return Err(Error::TooDeep {
                offset: self.p,
                limit: MAX_DEPTH,
            });
        }
        let mut f = Function {
            source: self.string()?,
            line_defined: self.i32()?,
            last_line_defined: self.i32()?,
            num_params: self.u8()?,
            is_vararg: self.u8()?,
            max_stack_size: self.u8()?,
            ..Function::default()
        };
        let n = self.count(4)?;
        f.code = (0..n).map(|_| self.u32()).collect::<Result<_>>()?;
        let n = self.count(1)?;
        for _ in 0..n {
            let at = self.p;
            let constant = match self.u8()? {
                0 => Constant::Nil,
                1 => Constant::Boolean(self.u8()? != 0),
                3 => Constant::Float(f64::from_bits(self.u64()?)),
                19 => Constant::Integer(self.u64()? as i64),
                tag @ (4 | 20) => {
                    let s = self.string()?.ok_or(Error::NullString { offset: at })?;
                    if tag == 4 {
                        Constant::ShortString(s)
                    } else {
                        Constant::LongString(s)
                    }
                }
                tag => return Err(Error::ConstantType { offset: at, tag }),
            };
            f.constants.push(constant);
        }
        let n = self.count(2)?;
        for _ in 0..n {
            f.upvalues.push(Upvalue {
                in_stack: self.u8()?,
                index: self.u8()?,
            });
        }
        let n = self.count(1)?;
        for _ in 0..n {
            f.protos.push(self.function(depth + 1)?);
        }
        let n = self.count(4)?;
        f.line_info = (0..n).map(|_| self.i32()).collect::<Result<_>>()?;
        let n = self.count(9)?;
        for _ in 0..n {
            f.loc_vars.push(LocVar {
                name: self.string()?,
                start_pc: self.i32()?,
                end_pc: self.i32()?,
            });
        }
        let n = self.count(1)?;
        for _ in 0..n {
            f.upvalue_names.push(self.string()?);
        }
        Ok(f)
    }
}

fn write_count(w: &mut Vec<u8>, n: usize) {
    // `parse` never yields 2^31 items; saturate rather than wrap for a hand-built function.
    w.extend_from_slice(&i32::try_from(n).unwrap_or(i32::MAX).to_le_bytes());
}

fn write_string(w: &mut Vec<u8>, s: Option<&[u8]>) {
    match s {
        None => w.push(0),
        Some(s) => {
            let size = s.len() as u64 + 1;
            if size < 0xFF {
                w.push(size as u8);
            } else {
                w.push(0xFF);
                w.extend_from_slice(&size.to_le_bytes());
            }
            w.extend_from_slice(s);
        }
    }
}

fn write_function(w: &mut Vec<u8>, f: &Function) {
    write_string(w, f.source.as_deref());
    w.extend_from_slice(&f.line_defined.to_le_bytes());
    w.extend_from_slice(&f.last_line_defined.to_le_bytes());
    w.extend_from_slice(&[f.num_params, f.is_vararg, f.max_stack_size]);
    write_count(w, f.code.len());
    for c in &f.code {
        w.extend_from_slice(&c.to_le_bytes());
    }
    write_count(w, f.constants.len());
    for c in &f.constants {
        match c {
            Constant::Nil => w.push(0),
            Constant::Boolean(b) => w.extend_from_slice(&[1, u8::from(*b)]),
            Constant::Float(x) => {
                w.push(3);
                w.extend_from_slice(&x.to_bits().to_le_bytes());
            }
            Constant::Integer(n) => {
                w.push(19);
                w.extend_from_slice(&n.to_le_bytes());
            }
            Constant::ShortString(s) => {
                w.push(4);
                write_string(w, Some(s));
            }
            Constant::LongString(s) => {
                w.push(20);
                write_string(w, Some(s));
            }
        }
    }
    write_count(w, f.upvalues.len());
    for u in &f.upvalues {
        w.extend_from_slice(&[u.in_stack, u.index]);
    }
    write_count(w, f.protos.len());
    for p in &f.protos {
        write_function(w, p);
    }
    write_count(w, f.line_info.len());
    for l in &f.line_info {
        w.extend_from_slice(&l.to_le_bytes());
    }
    write_count(w, f.loc_vars.len());
    for v in &f.loc_vars {
        write_string(w, v.name.as_deref());
        w.extend_from_slice(&v.start_pc.to_le_bytes());
        w.extend_from_slice(&v.end_pc.to_le_bytes());
    }
    write_count(w, f.upvalue_names.len());
    for n in &f.upvalue_names {
        write_string(w, n.as_deref());
    }
}

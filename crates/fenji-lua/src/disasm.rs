//! A text listing of a chunk, our ground truth for reading. One block per function, numbered by
//! its path in the prototype tree ("0", "0.1", …). One line per instruction, with its operands,
//! RK constants as `-1-k` (as `luac -l` prints them) plus the constant they name, and absolute
//! jump targets.
use std::fmt::Write as _;

use crate::{Chunk, Constant, Function, Instr, Mode, Op, is_k, k_index};

/// The listing of every function in `chunk`.
pub fn disassemble(chunk: &Chunk) -> String {
    let mut out = String::new();
    function(&mut out, &chunk.main, "0");
    out
}

fn function(out: &mut String, f: &Function, path: &str) {
    let _ = writeln!(
        out,
        "function {path} (params {}, vararg {}, stack {}, upvalues {}, constants {}, functions {}, instructions {})",
        f.num_params,
        f.is_vararg,
        f.max_stack_size,
        f.upvalues.len(),
        f.constants.len(),
        f.protos.len(),
        f.code.len()
    );
    for (pc, &word) in f.code.iter().enumerate() {
        let _ = writeln!(
            out,
            "  [{:>4}] {}",
            pc + 1,
            instruction(f, pc, Instr(word), path)
        );
    }
    for (n, p) in f.protos.iter().enumerate() {
        function(out, p, &format!("{path}.{n}"));
    }
}

fn instruction(f: &Function, pc: usize, i: Instr, path: &str) -> String {
    let Some(op) = i.op() else {
        return format!("OP?{:<7} {:#010x}", i.code(), i.0);
    };
    let mut notes = Vec::new();
    let operands = match op.mode() {
        Mode::Abx => {
            match op {
                Op::LoadK => notes.push(constant(f, i.bx())),
                Op::Closure => notes.push(format!("function {path}.{}", i.bx())),
                _ => {}
            }
            format!("{} {}", i.a(), i.bx())
        }
        Mode::AsBx => {
            notes.push(format!("to [{}]", pc as i64 + 2 + i64::from(i.sbx())));
            format!("{} {}", i.a(), i.sbx())
        }
        Mode::Ax => i.ax().to_string(),
        Mode::Abc => {
            let (b_rk, c_rk) = op.rk_operands();
            let mut show = |x: u32, rk: bool| {
                if rk && is_k(x) {
                    notes.push(constant(f, k_index(x)));
                    (-1 - i64::from(k_index(x))).to_string()
                } else {
                    x.to_string()
                }
            };
            let (b, c) = (show(i.b(), b_rk), show(i.c(), c_rk));
            if matches!(op, Op::GetTabUp | Op::TryTabUp | Op::TryLibUp) {
                notes.insert(0, format!("U{}", i.b()));
            } else if op == Op::SetTabUp {
                notes.insert(0, format!("U{}", i.a()));
            }
            format!("{} {b} {c}", i.a())
        }
    };
    let line = format!("{:<10} {operands}", op.name());
    if notes.is_empty() {
        line
    } else {
        format!("{line:<32}; {}", notes.join(" "))
    }
}

fn constant(f: &Function, k: u32) -> String {
    match f.constants.get(k as usize) {
        Some(Constant::Nil) => "nil".into(),
        Some(Constant::Boolean(b)) => b.to_string(),
        Some(Constant::Integer(n)) => n.to_string(),
        Some(Constant::Float(x)) => format!("{x:?}"),
        Some(Constant::ShortString(s) | Constant::LongString(s)) => quote(s),
        None => format!("K{k}?"),
    }
}

/// A string constant as a quoted literal. Valid UTF-8 stays as is (the client's UI text is
/// Chinese); quotes, backslashes and control characters are escaped; invalid bytes become \xNN.
fn quote(s: &[u8]) -> String {
    let mut out = String::from("\"");
    for chunk in s.utf8_chunks() {
        for ch in chunk.valid().chars() {
            match ch {
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\t' => out.push_str("\\t"),
                c if c.is_control() => {
                    let _ = write!(out, "\\u{{{:x}}}", u32::from(c));
                }
                c => out.push(c),
            }
        }
        for b in chunk.invalid() {
            let _ = write!(out, "\\x{b:02x}");
        }
    }
    out.push('"');
    out
}

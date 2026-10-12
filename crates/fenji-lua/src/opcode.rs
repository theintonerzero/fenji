//! Lua 5.3's opcodes and the seven the client's VM adds. The client keeps the stock numbers 0–46
//! (each of its 100,465 functions ends in 38, RETURN) and adds 47–53 (spike-0 6.17;
//! docs/re/m1-script-format.md §3–§4).

/// How an instruction's operands are packed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Abc,
    Abx,
    AsBx,
    Ax,
}

/// One opcode. Declaration order is the client's numbering, so `op as u8` is its code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Op {
    Move,
    LoadK,
    LoadKx,
    LoadBool,
    LoadNil,
    GetUpval,
    GetTabUp,
    GetTable,
    SetTabUp,
    SetUpval,
    SetTable,
    NewTable,
    SelfOp,
    Add,
    Sub,
    Mul,
    Mod,
    Pow,
    Div,
    IDiv,
    BAnd,
    BOr,
    BXor,
    Shl,
    Shr,
    Unm,
    BNot,
    Not,
    Len,
    Concat,
    Jmp,
    Eq,
    Lt,
    Le,
    Test,
    TestSet,
    Call,
    TailCall,
    Return,
    ForLoop,
    ForPrep,
    TForCall,
    TForLoop,
    SetList,
    Closure,
    VarArg,
    ExtraArg,
    // Client-only, 47–53.
    Nop,
    XGetTabUp,
    TryTabUp,
    TryLibUp,
    XGetSelf,
    XSelf,
    TrySelf,
}

const ALL: [Op; 54] = [
    Op::Move,
    Op::LoadK,
    Op::LoadKx,
    Op::LoadBool,
    Op::LoadNil,
    Op::GetUpval,
    Op::GetTabUp,
    Op::GetTable,
    Op::SetTabUp,
    Op::SetUpval,
    Op::SetTable,
    Op::NewTable,
    Op::SelfOp,
    Op::Add,
    Op::Sub,
    Op::Mul,
    Op::Mod,
    Op::Pow,
    Op::Div,
    Op::IDiv,
    Op::BAnd,
    Op::BOr,
    Op::BXor,
    Op::Shl,
    Op::Shr,
    Op::Unm,
    Op::BNot,
    Op::Not,
    Op::Len,
    Op::Concat,
    Op::Jmp,
    Op::Eq,
    Op::Lt,
    Op::Le,
    Op::Test,
    Op::TestSet,
    Op::Call,
    Op::TailCall,
    Op::Return,
    Op::ForLoop,
    Op::ForPrep,
    Op::TForCall,
    Op::TForLoop,
    Op::SetList,
    Op::Closure,
    Op::VarArg,
    Op::ExtraArg,
    Op::Nop,
    Op::XGetTabUp,
    Op::TryTabUp,
    Op::TryLibUp,
    Op::XGetSelf,
    Op::XSelf,
    Op::TrySelf,
];

/// The names in the client's own opcode-name table (spike-0 6.17), in code order.
const NAMES: [&str; 54] = [
    "MOVE",
    "LOADK",
    "LOADKX",
    "LOADBOOL",
    "LOADNIL",
    "GETUPVAL",
    "GETTABUP",
    "GETTABLE",
    "SETTABUP",
    "SETUPVAL",
    "SETTABLE",
    "NEWTABLE",
    "SELF",
    "ADD",
    "SUB",
    "MUL",
    "MOD",
    "POW",
    "DIV",
    "IDIV",
    "BAND",
    "BOR",
    "BXOR",
    "SHL",
    "SHR",
    "UNM",
    "BNOT",
    "NOT",
    "LEN",
    "CONCAT",
    "JMP",
    "EQ",
    "LT",
    "LE",
    "TEST",
    "TESTSET",
    "CALL",
    "TAILCALL",
    "RETURN",
    "FORLOOP",
    "FORPREP",
    "TFORCALL",
    "TFORLOOP",
    "SETLIST",
    "CLOSURE",
    "VARARG",
    "EXTRAARG",
    "NOP",
    "XGETTABUP",
    "TRYTABUP",
    "TRYLIBUP",
    "XGETSELF",
    "XSELF",
    "TRYSELF",
];

/// The number of stock Lua 5.3 opcodes.
pub const STOCK_COUNT: u8 = 47;

impl Op {
    /// The opcode whose client code is `code` (0–53).
    pub fn from_code(code: u8) -> Option<Op> {
        ALL.get(usize::from(code)).copied()
    }

    /// The client's code; equal to the stock code for 0–46.
    pub fn code(self) -> u8 {
        self as u8
    }

    pub fn is_stock(self) -> bool {
        self.code() < STOCK_COUNT
    }

    /// The name the client's opcode table uses.
    pub fn name(self) -> &'static str {
        NAMES[usize::from(self.code())]
    }

    /// How the operands are packed. `XGETTABUP` and `XSELF` carry a function-cache index in Bx
    /// (docs/re/m1-script-format.md §4).
    pub fn mode(self) -> Mode {
        match self {
            Op::LoadK | Op::LoadKx | Op::Closure | Op::XGetTabUp | Op::XSelf => Mode::Abx,
            Op::Jmp | Op::ForLoop | Op::ForPrep | Op::TForLoop => Mode::AsBx,
            Op::ExtraArg => Mode::Ax,
            _ => Mode::Abc,
        }
    }

    /// Whether B and C are RK operands: a register, or a constant when bit 8 is set. Of the
    /// client's extras, the three TRY opcodes read RK(C) like their stock namesakes, and the
    /// others have none (docs/re/m1-script-format.md §4).
    pub fn rk_operands(self) -> (bool, bool) {
        match self {
            Op::GetTabUp
            | Op::GetTable
            | Op::SelfOp
            | Op::TryTabUp
            | Op::TryLibUp
            | Op::TrySelf => (false, true),
            Op::SetTabUp
            | Op::SetTable
            | Op::Add
            | Op::Sub
            | Op::Mul
            | Op::Mod
            | Op::Pow
            | Op::Div
            | Op::IDiv
            | Op::BAnd
            | Op::BOr
            | Op::BXor
            | Op::Shl
            | Op::Shr
            | Op::Eq
            | Op::Lt
            | Op::Le => (true, true),
            _ => (false, false),
        }
    }
}

/// Bit 8 of a B or C operand marks a constant index (Lua 5.3's BITRK).
pub const BITRK: u32 = 1 << 8;

pub fn is_k(x: u32) -> bool {
    x & BITRK != 0
}

pub fn k_index(x: u32) -> u32 {
    x & !BITRK
}

/// One instruction, in Lua 5.3's layout: op 6 bits, A 8, C 9, B 9; Bx 18; sBx biased by 131,071.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Instr(pub u32);

impl Instr {
    pub fn code(self) -> u8 {
        (self.0 & 0x3F) as u8
    }

    pub fn op(self) -> Option<Op> {
        Op::from_code(self.code())
    }

    pub fn a(self) -> u32 {
        (self.0 >> 6) & 0xFF
    }

    pub fn c(self) -> u32 {
        (self.0 >> 14) & 0x1FF
    }

    pub fn b(self) -> u32 {
        (self.0 >> 23) & 0x1FF
    }

    pub fn bx(self) -> u32 {
        self.0 >> 14
    }

    pub fn sbx(self) -> i32 {
        (self.0 >> 14) as i32 - 131_071
    }

    pub fn ax(self) -> u32 {
        self.0 >> 6
    }

    /// The same operands under another opcode.
    pub fn with_op(self, op: Op) -> Instr {
        Instr((self.0 & !0x3F) | u32::from(op.code()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_and_names_line_up() {
        for code in 0..54u8 {
            let op = Op::from_code(code).unwrap();
            assert_eq!(op.code(), code);
            assert_eq!(op.is_stock(), code < 47);
        }
        assert_eq!(Op::from_code(54), None);
        assert_eq!((Op::Return.code(), Op::Return.name()), (38, "RETURN"));
        assert_eq!((Op::TryTabUp.code(), Op::TrySelf.name()), (49, "TRYSELF"));
    }

    #[test]
    fn instruction_fields() {
        let ret = Instr(38 | (1 << 23)); // RETURN 0 1: how every Lua 5.3 function ends
        assert_eq!(
            (ret.op(), ret.a(), ret.b(), ret.c()),
            (Some(Op::Return), 0, 1, 0)
        );
        assert_eq!(Instr(30 | ((131_071 - 1) << 14)).sbx(), -1); // JMP 0 -1
        assert_eq!(Instr(38).with_op(Op::Nop).code(), 47);
        assert!(is_k(BITRK | 3) && k_index(BITRK | 3) == 3 && !is_k(3));
    }

    #[test]
    fn extras_are_packed_the_way_the_vm_reads_them() {
        // m1-script-format §4: XGETTABUP and XSELF are A, Bx (a cache index); XGETSELF reads A
        // and B; the TRY opcodes read RK(C) like GETTABUP and SELF.
        assert_eq!(
            (Op::XGetTabUp.mode(), Op::XSelf.mode()),
            (Mode::Abx, Mode::Abx)
        );
        assert_eq!(
            (Op::XGetSelf.mode(), Op::Nop.mode()),
            (Mode::Abc, Mode::Abc)
        );
        for op in [Op::XGetTabUp, Op::XSelf, Op::XGetSelf, Op::Nop] {
            assert_eq!(op.rk_operands(), (false, false), "{}", op.name());
        }
        for op in [Op::TryTabUp, Op::TryLibUp, Op::TrySelf] {
            assert_eq!(
                op.rk_operands(),
                Op::GetTabUp.rk_operands(),
                "{}",
                op.name()
            );
        }
    }
}

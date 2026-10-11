//! The SoLO client's Lua 5.3 dialect (M1 spec §3): binary chunks in the stock and the client
//! dialect, a parser and a writer; later a disassembler, a normaliser to stock Lua 5.3 and a
//! census. Pure: no I/O.
mod census;
mod chunk;
mod disasm;
mod error;
mod normalize;
mod opcode;

pub use census::{Counts, census};
pub use chunk::{
    Chunk, Constant, Dialect, Function, LocVar, MAX_DEPTH, Upvalue, header, parse, write,
};
pub use disasm::disassemble;
pub use error::{Error, Result};
pub use normalize::{Action, Report, action, normalize};
pub use opcode::{BITRK, Instr, Mode, Op, STOCK_COUNT, is_k, k_index};

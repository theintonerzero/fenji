//! The client's dialect → stock Lua 5.3, for the external decompiler (M1 spec §3), following the
//! VM reading in docs/re/m1-script-format.md §4. `TRYTABUP` and `TRYLIBUP` become `GETTABUP`.
//! Each `TRYSELF`, a caching prefix to the identical `SELF` that follows it, is removed, and
//! jumps, line info and local-variable ranges are re-targeted. The run-time forms `NOP`,
//! `XGETTABUP`, `XGETSELF` and `XSELF` refuse the chunk. The disassembly stays the ground truth.
use std::collections::BTreeMap;

use crate::{Chunk, Error, Function, Instr, Op, Result};

/// What normalisation does with one opcode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// A stock opcode: unchanged.
    Keep,
    /// Becomes this stock opcode, with the same operands.
    Replace(Op),
    /// Removed: a prefix whose work the next instruction does in stock Lua.
    Remove,
    /// No stock form: the chunk is refused rather than decompiled wrongly.
    Refuse,
}

/// The action for `op`: the normalisation table of docs/re/m1-script-format.md §4.
pub fn action(op: Op) -> Action {
    match op {
        Op::TryTabUp | Op::TryLibUp => Action::Replace(Op::GetTabUp),
        Op::TrySelf => Action::Remove,
        Op::Nop | Op::XGetTabUp | Op::XGetSelf | Op::XSelf => Action::Refuse,
        _ => Action::Keep,
    }
}

/// What a normalisation changed: client opcode name → instructions replaced or removed.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Report {
    pub rewritten: BTreeMap<&'static str, u64>,
}

/// A stock Lua 5.3 copy of `chunk` (write it with `Dialect::Stock53`), and what changed.
pub fn normalize(chunk: &Chunk) -> Result<(Chunk, Report)> {
    let mut out = chunk.clone();
    let mut report = Report::default();
    rewrite(&mut out.main, &mut report)?;
    Ok((out, report))
}

const BIAS: i64 = 131_071;

fn has_sbx(op: Op) -> bool {
    matches!(op, Op::Jmp | Op::ForLoop | Op::ForPrep | Op::TForLoop)
}

/// Whether `i` skips or consumes the instruction after it, so that one can't be removed.
fn guards_next(i: Instr) -> bool {
    match i.op() {
        Some(Op::Eq | Op::Lt | Op::Le | Op::Test | Op::TestSet | Op::LoadKx | Op::TForCall) => true,
        Some(Op::LoadBool) => i.c() != 0,
        Some(Op::SetList) => i.c() == 0,
        _ => false,
    }
}

fn rewrite(f: &mut Function, report: &mut Report) -> Result<()> {
    let n = f.code.len();
    // removed_before[pc]: instructions removed before `pc`, for every pc in 0..=n.
    let mut removed_before = Vec::with_capacity(n + 1);
    let mut kept: Vec<(usize, u32)> = Vec::with_capacity(n);
    let mut removed = 0;
    for pc in 0..n {
        removed_before.push(removed);
        let i = Instr(f.code[pc]);
        let op = i.op().ok_or(Error::UnknownOpcode { code: i.code() })?;
        match action(op) {
            Action::Keep => kept.push((pc, i.0)),
            Action::Replace(stock) => {
                kept.push((pc, i.with_op(stock).0));
                *report.rewritten.entry(op.name()).or_default() += 1;
            }
            Action::Remove => {
                let next = f.code.get(pc + 1).map(|&w| Instr(w));
                if !next.is_some_and(|s| s.op() == Some(Op::SelfOp) && s.0 >> 6 == i.0 >> 6) {
                    return Err(Error::Unremovable {
                        pc,
                        reason: "not followed by the identical SELF",
                    });
                }
                if pc > 0 && guards_next(Instr(f.code[pc - 1])) {
                    return Err(Error::Unremovable {
                        pc,
                        reason: "follows an instruction that skips or consumes it",
                    });
                }
                removed += 1;
                *report.rewritten.entry(op.name()).or_default() += 1;
            }
            Action::Refuse => return Err(Error::Unmapped { op: op.name() }),
        }
    }
    removed_before.push(removed);
    if removed == 0 {
        f.code = kept.into_iter().map(|(_, word)| word).collect();
    } else {
        let new = |pc: usize| pc - removed_before[pc];
        let mut code = Vec::with_capacity(kept.len());
        for (pc, word) in kept {
            let i = Instr(word);
            if i.op().is_some_and(has_sbx) {
                let target = pc as i64 + 1 + i64::from(i.sbx());
                let target =
                    usize::try_from(target)
                        .ok()
                        .filter(|&t| t <= n)
                        .ok_or(Error::Unremovable {
                            pc,
                            reason: "a jump leaves the function",
                        })?;
                let sbx = new(target) as i64 - new(pc) as i64 - 1;
                code.push((word & 0x3FFF) | (((sbx + BIAS) as u32) << 14));
            } else {
                code.push(word);
            }
        }
        if !f.line_info.is_empty() {
            if f.line_info.len() != n {
                return Err(Error::Unremovable {
                    pc: 0,
                    reason: "line info doesn't match the code",
                });
            }
            let lines = std::mem::take(&mut f.line_info);
            f.line_info = lines
                .into_iter()
                .enumerate()
                .filter(|&(pc, _)| !is_removed(&removed_before, pc))
                .map(|(_, l)| l)
                .collect();
        }
        for v in &mut f.loc_vars {
            for pc in [&mut v.start_pc, &mut v.end_pc] {
                let old = usize::try_from(*pc).ok().filter(|&p| p <= n);
                let old = old.ok_or(Error::Unremovable {
                    pc: 0,
                    reason: "a local variable's range leaves the function",
                })?;
                *pc = new(old) as i32;
            }
        }
        f.code = code;
    }
    f.protos.iter_mut().try_for_each(|p| rewrite(p, report))
}

/// Whether the instruction at `pc` was removed: the count of removals grows right after it.
fn is_removed(removed_before: &[usize], pc: usize) -> bool {
    removed_before[pc + 1] > removed_before[pc]
}

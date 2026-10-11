mod common;

use fenji_lua::{
    Action, Chunk, Dialect, Error, Function, Instr, Op, action, is_k, normalize, parse, write,
};

/// The opcodes with an sBx jump.
const SBX: [Op; 4] = [Op::Jmp, Op::ForLoop, Op::ForPrep, Op::TForLoop];

fn with_sbx(i: Instr, sbx: i64) -> u32 {
    (i.0 & 0x3FFF) | (((sbx + 131_071) as u32) << 14)
}

/// The jump target of an sBx instruction at `pc`.
fn target(pc: usize, i: Instr) -> usize {
    (pc as i64 + 1 + i64::from(i.sbx())) as usize
}

/// Stock code in the form the client's compiler emits (docs/re/m1-script-format.md §4): a TRYSELF
/// in front of every SELF, with jumps that landed on the SELF landing on the TRYSELF; GETTABUP
/// followed by GETTABLE A A K as TRYLIBUP; any other GETTABUP as TRYTABUP. Line info and local
/// variable ranges follow. Returns how many instructions it rewrote or added.
fn clientify(f: &mut Function) -> u64 {
    let old = std::mem::take(&mut f.code);
    let mut added_before = Vec::with_capacity(old.len() + 1);
    let mut added = 0;
    for &w in &old {
        added_before.push(added);
        added += usize::from(Instr(w).op() == Some(Op::SelfOp));
    }
    added_before.push(added);
    let new = |pc: usize| pc + added_before[pc];
    let mut changed = 0;
    for (pc, &w) in old.iter().enumerate() {
        let i = Instr(w);
        let word = match i.op() {
            Some(Op::SelfOp) => {
                f.code.push(i.with_op(Op::TrySelf).0);
                changed += 1;
                w
            }
            Some(Op::GetTabUp) => {
                let next = old.get(pc + 1).map(|&n| Instr(n));
                let lib = next.is_some_and(|n| {
                    n.op() == Some(Op::GetTable) && n.a() == i.a() && n.b() == i.a() && is_k(n.c())
                });
                changed += 1;
                i.with_op(if lib { Op::TryLibUp } else { Op::TryTabUp }).0
            }
            Some(op) if SBX.contains(&op) => {
                with_sbx(i, new(target(pc, i)) as i64 - new(pc) as i64 - 1)
            }
            _ => w,
        };
        f.code.push(word);
    }
    if !f.line_info.is_empty() {
        let lines = std::mem::take(&mut f.line_info);
        for (pc, line) in lines.into_iter().enumerate() {
            if Instr(old[pc]).op() == Some(Op::SelfOp) {
                f.line_info.push(line);
            }
            f.line_info.push(line);
        }
    }
    for v in &mut f.loc_vars {
        v.start_pc = new(v.start_pc as usize) as i32;
        v.end_pc = new(v.end_pc as usize) as i32;
    }
    changed + f.protos.iter_mut().map(clientify).sum::<u64>()
}

/// How many sBx jumps in `f` and its nested functions land on a SELF.
fn jumps_onto_self(f: &Function) -> usize {
    let here = f
        .code
        .iter()
        .enumerate()
        .filter(|&(pc, &w)| {
            let i = Instr(w);
            i.op().is_some_and(|op| SBX.contains(&op))
                && Instr(f.code[target(pc, i)]).op() == Some(Op::SelfOp)
        })
        .count();
    here + f.protos.iter().map(jumps_onto_self).sum::<usize>()
}

#[test]
fn the_action_table_is_m1_script_format_section_4() {
    assert_eq!(action(Op::TryTabUp), Action::Replace(Op::GetTabUp));
    assert_eq!(action(Op::TryLibUp), Action::Replace(Op::GetTabUp));
    assert_eq!(action(Op::TrySelf), Action::Remove);
    for op in [Op::Nop, Op::XGetTabUp, Op::XGetSelf, Op::XSelf] {
        assert_eq!(action(op), Action::Refuse, "{}", op.name());
    }
    for code in 0..47 {
        assert_eq!(action(Op::from_code(code).unwrap()), Action::Keep);
    }
}

#[test]
fn the_fixtures_jump_onto_method_calls() {
    // Without this, the round trip below wouldn't exercise re-targeting through a removed TRYSELF.
    let chunk = parse(&common::luac(common::METHODS, true), Dialect::Stock53).unwrap();
    assert!(jumps_onto_self(&chunk.main) >= 2);
}

#[test]
fn normalising_the_clients_form_gives_back_the_stock_bytes() {
    let mut removed = 0;
    for stock in common::fixtures() {
        let mut chunk = parse(&stock, Dialect::Stock53).unwrap();
        let changed = clientify(&mut chunk.main);
        let client = parse(&write(&chunk, Dialect::Client), Dialect::Client).unwrap();
        let (normal, report) = normalize(&client).unwrap();
        assert_eq!(write(&normal, Dialect::Stock53), stock);
        assert_eq!(report.rewritten.values().sum::<u64>(), changed);
        removed += report.rewritten.get("TRYSELF").copied().unwrap_or(0);
    }
    assert!(removed > 0, "the fixtures must contain method calls");
}

#[test]
fn a_tryself_without_its_self_refuses_the_chunk() {
    let mut chunk = parse(&common::luac(common::METHODS, true), Dialect::Stock53).unwrap();
    clientify(&mut chunk.main);
    let pc = chunk
        .main
        .code
        .iter()
        .position(|&w| Instr(w).op() == Some(Op::TrySelf))
        .unwrap();
    chunk.main.code[pc + 1] ^= 1 << 14; // the SELF's C no longer matches
    assert!(matches!(normalize(&chunk), Err(Error::Unremovable { pc: p, .. }) if p == pc));
}

#[test]
fn a_tryself_after_a_skipping_instruction_refuses_the_chunk() {
    // EQ skips the next instruction; removing a TRYSELF there would change what it skips.
    let self_op = Instr(u32::from(Op::SelfOp.code()) | (1 << 6));
    let ret = 38 | (1 << 23);
    let code = vec![
        u32::from(Op::Eq.code()),
        self_op.with_op(Op::TrySelf).0,
        self_op.0,
        ret,
    ];
    let chunk = Chunk {
        upvalue_count: 0,
        main: Function {
            code,
            ..Function::default()
        },
    };
    assert!(matches!(
        normalize(&chunk),
        Err(Error::Unremovable { pc: 1, .. })
    ));
}

#[test]
fn unmapped_opcode_refuses_the_chunk() {
    let mut chunk = parse(&common::luac("print('hi')", true), Dialect::Stock53).unwrap();
    chunk.main.code[0] = Instr(chunk.main.code[0]).with_op(Op::Nop).0;
    assert_eq!(
        normalize(&chunk).unwrap_err(),
        Error::Unmapped { op: "NOP" }
    );
}

#[test]
fn unknown_code_refuses_the_chunk() {
    let mut chunk = parse(&common::luac("print('hi')", true), Dialect::Stock53).unwrap();
    chunk.main.code.insert(0, 60);
    assert_eq!(
        normalize(&chunk).unwrap_err(),
        Error::UnknownOpcode { code: 60 }
    );
}

mod common;

use fenji_lua::{Dialect, Function, Instr, Op, disassemble, parse};

fn instructions(f: &Function) -> usize {
    f.code.len() + f.protos.iter().map(instructions).sum::<usize>()
}

#[test]
fn lists_every_function_and_instruction() {
    let chunk = parse(&common::luac(common::MODULE, true), Dialect::Stock53).unwrap();
    let text = disassemble(&chunk);
    assert_eq!(
        text.lines().filter(|l| l.starts_with("  [")).count(),
        instructions(&chunk.main)
    );
    assert!(text.starts_with("function 0 ("));
    assert!(text.contains("function 0.0 ")); // M.pick
    assert!(text.contains("\"ipairs\""));
    assert!(text.contains("\"IsServer\""));
    assert!(text.contains("; to ["));
}

#[test]
fn client_opcodes_and_unknown_codes_are_shown() {
    let mut chunk = parse(&common::luac("print('hi')", true), Dialect::Stock53).unwrap();
    let first = Instr(chunk.main.code[0]);
    assert_eq!(first.op(), Some(Op::GetTabUp));
    chunk.main.code[0] = first.with_op(Op::TryTabUp).0;
    chunk.main.code.push(60); // in neither table
    let text = disassemble(&chunk);
    assert!(text.contains("TRYTABUP"));
    assert!(text.contains("\"print\""));
    assert!(text.contains("OP?60"));
}

#[test]
fn strings_are_escaped_but_utf8_is_kept() {
    let chunk = parse(
        &common::luac("return 'a\"b\\n', '\\xff', '中文'", true),
        Dialect::Stock53,
    )
    .unwrap();
    let text = disassemble(&chunk);
    assert!(text.contains(r#""a\"b\n""#));
    assert!(text.contains(r#""\xff""#));
    assert!(text.contains("\"中文\""));
}

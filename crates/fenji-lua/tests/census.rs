mod common;

use fenji_lua::{Counts, Dialect, Instr, Op, census, parse};

fn counts(src: &str) -> Counts {
    census(&parse(&common::luac(src, true), Dialect::Stock53).unwrap())
}

#[test]
fn server_and_client_blocks_are_counted() {
    let c = counts(
        "local x = 0\nif IsServer() then x = x + 1 x = x * 2 end\nif IsClient() then x = 7 end\nreturn x",
    );
    assert_eq!(
        (c.chunks, c.functions, c.reads_is_server, c.reads_is_client),
        (1, 1, 1, 1)
    );
    assert_eq!((c.server_only, c.client_only), (2, 1), "{c:?}");
}

#[test]
fn negated_guards_swap_sides() {
    let plain = counts("local x = 0 if IsServer() then x = 1 end return x");
    let negated = counts("local x = 0 if not IsServer() then x = 1 end return x");
    assert_eq!((plain.server_only, plain.client_only), (1, 0));
    assert_eq!((negated.server_only, negated.client_only), (0, 1));
}

#[test]
fn an_early_return_guards_the_rest_of_the_function() {
    // `if not IsServer() then return end`: everything after it runs only on the server.
    let c = counts("local x = 0 if not IsServer() then return end x = 1 x = 2 return x");
    assert_eq!(c.client_only, 1, "the RETURN itself");
    assert!(c.server_only >= 3, "{c:?}");
}

#[test]
fn nested_functions_are_visited_and_unguarded_code_is_neither() {
    let c = counts(
        "local function f() if IsServer() then return 1 end end local function g() return 2 end return f, g",
    );
    assert_eq!((c.functions, c.reads_is_server), (3, 1));
    let plain = counts("local function g() return 2 end return g");
    assert_eq!((plain.server_only, plain.client_only), (0, 0));
}

#[test]
fn the_clients_global_reads_count_too() {
    let mut chunk = parse(
        &common::luac("if IsServer() then print(1) end", true),
        Dialect::Stock53,
    )
    .unwrap();
    for w in &mut chunk.main.code {
        let i = Instr(*w);
        if i.op() == Some(Op::GetTabUp) {
            *w = i.with_op(Op::TryTabUp).0;
        }
    }
    let c = census(&chunk);
    assert_eq!(c.reads_is_server, 1);
    assert!(c.server_only > 0);
}

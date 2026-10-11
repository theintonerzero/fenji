//! Server-side logic in the client's scripts, measured from the bytecode (M1 spec §4, W6). The
//! measure is syntactic. It finds `if IsServer() then … end` (and IsClient, and `not` of either):
//! a read of the global, `CALL A 1 2`, `TEST A k` and a forward `JMP`. The instructions the JMP
//! skips run only when the test passes. When that block is a lone RETURN (`if not IsServer() then
//! return end`), the rest of the function runs only in the opposite case.
use std::collections::BTreeSet;
use std::ops::{AddAssign, Range};

use crate::{Chunk, Constant, Function, Instr, Op, is_k, k_index};

/// Counts for one chunk, or summed over many.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Counts {
    pub chunks: u64,
    pub functions: u64,
    pub instructions: u64,
    /// Functions that read the global `IsServer` or `IsClient`.
    pub reads_is_server: u64,
    pub reads_is_client: u64,
    /// Instructions that run only on the server: under `IsServer()` true or `IsClient()` false.
    pub server_only: u64,
    /// Instructions that run only on the client: under `IsClient()` true or `IsServer()` false.
    pub client_only: u64,
}

impl AddAssign for Counts {
    fn add_assign(&mut self, o: Self) {
        self.chunks += o.chunks;
        self.functions += o.functions;
        self.instructions += o.instructions;
        self.reads_is_server += o.reads_is_server;
        self.reads_is_client += o.reads_is_client;
        self.server_only += o.server_only;
        self.client_only += o.client_only;
    }
}

/// The counts for one chunk.
pub fn census(chunk: &Chunk) -> Counts {
    let mut c = Counts {
        chunks: 1,
        ..Counts::default()
    };
    visit(&chunk.main, &mut c);
    c
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Guard {
    IsServer,
    IsClient,
}

fn visit(f: &Function, c: &mut Counts) {
    c.functions += 1;
    c.instructions += f.code.len() as u64;
    let (mut server, mut client) = (BTreeSet::new(), BTreeSet::new());
    let (mut reads_server, mut reads_client) = (false, false);
    for pc in 0..f.code.len() {
        let Some(guard) = global_read(f, pc) else {
            continue;
        };
        match guard {
            Guard::IsServer => reads_server = true,
            Guard::IsClient => reads_client = true,
        }
        if let Some((block, when_true)) = guarded_block(f, pc) {
            let on_server = (guard == Guard::IsServer) == when_true;
            let early_return =
                block.len() == 1 && Instr(f.code[block.start]).op() == Some(Op::Return);
            let rest = block.end..f.code.len();
            let (block_side, rest_side) = if on_server {
                (&mut server, &mut client)
            } else {
                (&mut client, &mut server)
            };
            block_side.extend(block);
            if early_return {
                rest_side.extend(rest);
            }
        }
    }
    c.reads_is_server += u64::from(reads_server);
    c.reads_is_client += u64::from(reads_client);
    c.server_only += server.len() as u64;
    c.client_only += client.len() as u64;
    for p in &f.protos {
        visit(p, c);
    }
}

/// `R(A) := Up[B][K(C)]` where K(C) is the string "IsServer" or "IsClient". That is how a global
/// read compiles, as GETTABUP or the client's TRYTABUP, TRYLIBUP or XGETTABUP.
fn global_read(f: &Function, pc: usize) -> Option<Guard> {
    let i = Instr(f.code[pc]);
    if !matches!(
        i.op()?,
        Op::GetTabUp | Op::TryTabUp | Op::TryLibUp | Op::XGetTabUp
    ) || !is_k(i.c())
    {
        return None;
    }
    match f.constants.get(k_index(i.c()) as usize)? {
        Constant::ShortString(s) if s == b"IsServer" => Some(Guard::IsServer),
        Constant::ShortString(s) if s == b"IsClient" => Some(Guard::IsClient),
        _ => None,
    }
}

/// After the read at `pc`: `CALL A 1 2`, `TEST A k`, `JMP 0 +n`. The n instructions after the
/// JMP run only when the result is truthy (k = 0) or falsy (k = 1). Returns them, and which case.
fn guarded_block(f: &Function, pc: usize) -> Option<(Range<usize>, bool)> {
    let a = Instr(f.code[pc]).a();
    let at = |n: usize| f.code.get(pc + n).map(|&w| Instr(w));
    let (call, test, jmp) = (at(1)?, at(2)?, at(3)?);
    let shape = call.op() == Some(Op::Call)
        && call.a() == a
        && call.b() == 1
        && call.c() == 2
        && test.op() == Some(Op::Test)
        && test.a() == a
        && jmp.op() == Some(Op::Jmp)
        && jmp.sbx() > 0;
    if !shape {
        return None;
    }
    let start = pc + 4;
    let end = (start + jmp.sbx() as usize).min(f.code.len());
    Some((start..end, test.c() == 0))
}

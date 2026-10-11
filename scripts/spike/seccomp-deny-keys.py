#!/usr/bin/env python3
"""Builds the seccomp filter isolated-run.sh hands to `bwrap --seccomp FD`.

The filter makes add_key, request_key and keyctl fail with EPERM. request_key (and keyctl paths
that end in it) can make the kernel run /sbin/request-key as root in the host's namespaces with
arguments the caller chose, and /etc/request-key.conf maps key types to host programs: a DNS lookup
for dns_resolver, cifs.upcall, nfsidmap. That is a host process acting for the sandbox, so it has
to be impossible. The sandbox also shares the host's session and user keyrings; with keyctl gone
it can't read or change them.

Classic BPF over struct seccomp_data { int nr; __u32 arch; ... }, packed as struct sock_filter
{ __u16 code; __u8 jt; __u8 jf; __u32 k; } in native byte order. Standard library only.

  arch == AUDIT_ARCH_X86_64:
    nr & __X32_SYSCALL_BIT (nr >= 0x40000000)  -> EPERM   x32 ABI: denied wholesale. Nothing we
                                                          run uses it, and most kernels lack it.
    nr in {248 add_key, 249 request_key, 250 keyctl} -> EPERM
    otherwise                                   -> ALLOW
  arch == AUDIT_ARCH_I386 (32-bit processes, int 0x80 from 64-bit ones):
    nr in {286 add_key, 287 request_key, 288 keyctl} -> EPERM
    otherwise                                   -> ALLOW
  any other arch                                -> KILL_PROCESS

An x86_64 kernel only ever reports those two arch values, so the last rule never fires there. On
any other kernel the numbers above would mean other syscalls, so the filter kills rather than
guesses (fail closed); isolated-run.sh also refuses to start on non-x86_64 hosts.

Filters stack: a nested bwrap or pressure-vessel inside adds its own and ours stays in force.

Usage:
  seccomp-deny-keys.py            raw filter bytes on stdout (for an fd)
  seccomp-deny-keys.py --printf   the same bytes as \\0NNN escapes, for bash `printf '%b'`
  seccomp-deny-keys.py --disasm   human-readable listing
"""

import struct
import sys

AUDIT_ARCH_X86_64 = 0xC000003E
AUDIT_ARCH_I386 = 0x40000003
X32_SYSCALL_BIT = 0x40000000

SECCOMP_RET_KILL_PROCESS = 0x80000000
SECCOMP_RET_ERRNO = 0x00050000
SECCOMP_RET_ALLOW = 0x7FFF0000
EPERM = 1

# offsetof(struct seccomp_data, ...)
OFF_NR = 0
OFF_ARCH = 4

BPF_LD_W_ABS = 0x20  # BPF_LD | BPF_W | BPF_ABS
BPF_JEQ_K = 0x15  # BPF_JMP | BPF_JEQ | BPF_K
BPF_JGE_K = 0x35  # BPF_JMP | BPF_JGE | BPF_K
BPF_RET_K = 0x06  # BPF_RET | BPF_K

X86_64_DENY = {248: "add_key", 249: "request_key", 250: "keyctl"}
I386_DENY = {286: "add_key", 287: "request_key", 288: "keyctl"}


def build():
    """Returns the program as a list of (code, jt, jf, k, comment)."""
    # Layout (indices):            jumps are relative: target = index + 1 + jt/jf
    #  0 ld arch
    #  1 jeq X86_64  -> 4
    #  2 jeq I386    -> 10
    #  3 ret KILL
    #  4 ld nr                     (x86_64 block)
    #  5 jge X32_BIT -> deny
    #  6..8 jeq 248/249/250 -> deny
    #  9 ret ALLOW
    # 10 ld nr                     (i386 block)
    # 11..13 jeq 286/287/288 -> deny
    # 14 ret ALLOW
    # 15 ret ERRNO(EPERM)          (deny)
    deny = 15
    p = [
        (BPF_LD_W_ABS, 0, 0, OFF_ARCH, "load arch"),
        (BPF_JEQ_K, 4 - 2, 0, AUDIT_ARCH_X86_64, "x86_64?"),
        (BPF_JEQ_K, 10 - 3, 0, AUDIT_ARCH_I386, "i386?"),
        (BPF_RET_K, 0, 0, SECCOMP_RET_KILL_PROCESS, "unknown arch: kill"),
        (BPF_LD_W_ABS, 0, 0, OFF_NR, "x86_64: load nr"),
        (BPF_JGE_K, deny - 6, 0, X32_SYSCALL_BIT, "x32 ABI: deny"),
    ]
    for nr, name in X86_64_DENY.items():
        p.append((BPF_JEQ_K, deny - (len(p) + 1), 0, nr, f"x86_64 {name}: deny"))
    p.append((BPF_RET_K, 0, 0, SECCOMP_RET_ALLOW, "x86_64: allow"))
    assert len(p) == 10
    p.append((BPF_LD_W_ABS, 0, 0, OFF_NR, "i386: load nr"))
    for nr, name in I386_DENY.items():
        p.append((BPF_JEQ_K, deny - (len(p) + 1), 0, nr, f"i386 {name}: deny"))
    p.append((BPF_RET_K, 0, 0, SECCOMP_RET_ALLOW, "i386: allow"))
    assert len(p) == deny
    p.append((BPF_RET_K, 0, 0, SECCOMP_RET_ERRNO | EPERM, "deny: EPERM"))
    for i, (code, jt, jf, _, _) in enumerate(p):
        if code in (BPF_JEQ_K, BPF_JGE_K):
            assert 0 <= jt < 256 and 0 <= jf < 256 and i + 1 + max(jt, jf) < len(p)
    return p


def pack(prog):
    return b"".join(struct.pack("=HBBI", code, jt, jf, k) for code, jt, jf, k, _ in prog)


def disasm(prog):
    names = {BPF_LD_W_ABS: "ld", BPF_JEQ_K: "jeq", BPF_JGE_K: "jge", BPF_RET_K: "ret"}
    out = []
    for i, (code, jt, jf, k, comment) in enumerate(prog):
        op = names[code]
        if code in (BPF_JEQ_K, BPF_JGE_K):
            args = f"#{k:#010x} jt {i + 1 + jt} jf {i + 1 + jf}"
        elif code == BPF_LD_W_ABS:
            args = f"[{k}]"
        else:
            args = f"#{k:#010x}"
        out.append(f"{i:3} {op:4} {args:30} ; {comment}")
    return "\n".join(out) + "\n"


def main(argv):
    prog = build()
    if argv == []:
        if sys.stdout.isatty():
            sys.exit("seccomp-deny-keys.py: refusing to write binary to a terminal (try --disasm)")
        sys.stdout.buffer.write(pack(prog))
    elif argv == ["--printf"]:
        sys.stdout.write("".join(f"\\0{b:03o}" for b in pack(prog)))
    elif argv == ["--disasm"]:
        sys.stdout.write(disasm(prog))
    else:
        sys.exit("usage: seccomp-deny-keys.py [--printf | --disasm]")


if __name__ == "__main__":
    main(sys.argv[1:])

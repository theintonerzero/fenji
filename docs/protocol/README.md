# Protocol findings

Our own written findings about the pinned client's network protocol. No decompiled code or extracted scripts here: prose and tables only. Each fact cites its evidence: REA Evidence IDs, or pinned-exe addresses where REA can't reach.

## Files (created as milestones reach them)

| File | Contents | Milestone |
|---|---|---|
| [`transport.md`](transport.md) | Framing, header scramble, Oodle Network codec, gate handshake and variant selection | M2 |
| [`opcodes.md`](opcodes.md) | Opcode ledger (`STC_PROTOCOL_*`, `CTS_PROTOCOL_*`), with wire values; script message ids | M2 onward |
| [`payloads.md`](payloads.md) | The client's pickle format (script messages, the auth payload), compressed or not | M2 |
| `properties.md` | The synced-property (`prop_*`) schema, and how sync packets carry it | M2–M4 |
| [`packets/lobby.md`](packets/lobby.md) | The lobby's script messages: the character list, creating and deleting characters, error codes, close reasons, and what Fenji sends | M3 |
| `packets/<area>.md` | Per-opcode payload tables (`login`, `lobby`, `world`, `combat`, …) | M2–M5 |

## Opcode ledger format (`opcodes.md`)

| Value | Direction | Client name | Handler | Status | Evidence |
|---|---|---|---|---|---|
| `0` | C→S / S→C | `CTS_PROTOCOL_…` | native address, Lua script or `—` | `unknown` / `named` / `schema` / `verified` | Evidence |

Status meanings:

- `unknown`: value seen, purpose not known.
- `named`: the client's own name is known; the payload isn't mapped.
- `schema`: payload mapped from the client's code or scripts.
- `verified`: confirmed against the real client in a sandboxed stub session, and covered by a golden test.

## Payload table format (`packets/<area>.md`)

Script messages use the client's own pickle format ([`payloads.md`](payloads.md)); the other opcodes have per-type native layouts ([`opcodes.md`](opcodes.md)). For each opcode, give:
- the value and direction, and a one-line purpose;
- a field table (`key`, `type`, `meaning`, `notes`);
- the evidence, and the golden-test fixture name if one exists.

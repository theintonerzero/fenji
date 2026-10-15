// Until the screens come (launcher plan Task 12), the window shows the model's snapshot as text.
import { onSnapshot, snapshot } from "./ipc";
import type { Snapshot } from "./types";

const pre = document.createElement("pre");
document.body.append(pre);

function show(s: Snapshot): void {
  // textContent, never innerHTML: a server's words are untrusted (plan Global Constraints).
  pre.textContent = JSON.stringify(s, null, 2);
}

show(await snapshot());
await onSnapshot(show);

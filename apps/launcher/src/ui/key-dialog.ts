// A link offered another key for a saved server (hosting spec §3.10; launcher Task 4's ruling):
// nothing changes until the player chooses. A centred dialog over the stage (R7); "Use the new key"
// is its one ember action, and Escape keeps the saved key.
import type { Ctx } from "../app";
import type { Snapshot } from "../types";
import { button } from "./button";
import { dialog } from "./dialog";

/** Shown only while the snapshot holds a key change. */
export function keyDialog(ctx: Ctx, s: Snapshot): HTMLElement & { close(): void } {
  const change = s.keyChange!;
  const host = s.servers.find((v) => v.id === change.serverId)?.host ?? "your host";
  const shown = dialog({
    title: change.hadKey ? `A new key for ${change.name}` : `A key for ${change.name}`,
    description: change.hadKey
      ? `This link carries a different key. Use it only if ${host} told you the key changed.`
      : `This link carries the server's key. Use it only if it came from ${host}.`,
    actions: [
      button.secondary(change.hadKey ? "Keep the old key" : "Not now", () => void ctx.send({ type: "keepKey" })),
      button.primary(change.hadKey ? "Use the new key" : "Use this key", () =>
        void ctx.send({ type: "useNewKey", id: change.serverId }),
      ),
    ],
    onClose: () => void ctx.send({ type: "keepKey" }),
  });
  return Object.assign(shown.el, { close: shown.close });
}

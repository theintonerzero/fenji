// A link offered another key for a saved server (hosting spec §3.10; launcher Task 4's ruling):
// nothing changes until the player chooses. In the Add server dialog's style, over any screen;
// "Use the new key" is the one ember button, and Escape keeps the saved key.
import type { Ctx } from "../app";
import type { Snapshot } from "../types";
import { button } from "./button";
import { dialog } from "./dialog";
import { icons } from "./icons";

/** Shown only while the snapshot holds a key change. */
export function keyDialog(ctx: Ctx, s: Snapshot): HTMLElement & { close(): void } {
  const change = s.keyChange!;
  const host = s.servers.find((v) => v.id === change.serverId)?.host ?? "your host";
  const shown = dialog({
    title: change.hadKey ? `A different key for ${change.name}` : `A key for ${change.name}`,
    icon: icons.lock(20),
    description: change.hadKey
      ? `This link carries a different key for ${change.name} than the one saved. Use it only if ${host} told you the server's key changed.`
      : `This link carries a key for ${change.name}, which was saved without one. Use it only if the link came from ${host}.`,
    actions: [
      button.secondary("Keep the saved key", () => void ctx.send({ type: "keepKey" })),
      button.primary("Use the new key", () => void ctx.send({ type: "useNewKey", id: change.serverId })),
    ],
    onClose: () => void ctx.send({ type: "keepKey" }),
  });
  return Object.assign(shown.el, { close: shown.close });
}

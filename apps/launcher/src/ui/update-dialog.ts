// The update dialog (launcher spec §3.8; the Update board): once per downloaded version, over any
// screen. The notes are the release's own words, shown as text, a line to a bullet. "Restart and
// update" is its one ember button: the board's second ember is fixed (spec §3.10).
import type { Ctx } from "../app";
import { h } from "../dom";
import type { Snapshot } from "../types";
import { button } from "./button";
import { dialog } from "./dialog";
import { icons } from "./icons";

/** The notes, a line to a bullet, without the list marks a release may write. */
function lines(notes: string | null): string[] {
  return (notes ?? "")
    .split("\n")
    .map((line) => line.replace(/^\s*[-*•]\s+/, "").trim())
    .filter((line) => line !== "");
}

export function updateDialog(ctx: Ctx, s: Snapshot): HTMLElement & { close(): void } {
  const { update } = s;
  const next = update.available ?? "";
  const notes = lines(update.notes);
  const signed = icons.shield(16);
  signed.classList.add("icon-ok");
  const shown = dialog({
    title: `Fenji launcher ${next} is ready`,
    icon: icons.update(20),
    description: `It's downloaded. You have ${update.current}; restarting installs ${next} and opens the launcher again.`,
    body: [
      notes.length === 0
        ? null
        : [
            h("h3", { class: "dialog-subtitle" }, "What's new"),
            h(
              "ul",
              { class: "bullets dialog-bullets" },
              notes.map((line) => h("li", {}, h("span", {}, line))),
            ),
          ],
      h(
        "p",
        { class: "signed" },
        signed,
        "Signed with Fenji's release key. The launcher checks the signature before it installs.",
      ),
    ],
    actions: [
      button.secondary("Later", () => void ctx.send({ type: "dismissUpdate" })),
      button.primary("Restart and update", () => void ctx.send({ type: "installUpdate" })),
    ],
    onClose: () => void ctx.send({ type: "dismissUpdate" }),
  });
  return Object.assign(shown.el, { close: shown.close });
}

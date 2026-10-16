// DepotDownloader in the player's own terminal (launcher spec §3.6; the redesign's R9 keeps this
// path until the launcher runs it itself): one line with their folder and the pinned build filled
// in, their own Steam sign-in left for them to type. Without Fenji's own copy of the tool, where
// to get it first. Then they choose the folder it downloaded to, or verify the files it repaired.
import type { Ctx } from "../app";
import { h } from "../dom";
import type { ToolLinkView } from "../types";
import { button } from "./button";
import { dialog, type Dialog } from "./dialog";

/** The command, selected, for a player to copy by hand when the clipboard isn't there. */
function select(el: Element): void {
  const range = document.createRange();
  range.selectNodeContents(el);
  const selection = window.getSelection();
  selection?.removeAllRanges();
  selection?.addRange(range);
}

export function commandDialog(
  ctx: Ctx,
  command: string,
  get: ToolLinkView | null,
  then: "chooseFolder" | "check",
  done: () => void,
): Dialog {
  const code = h("code", {}, command);
  const copy = button.secondary(
    "Copy",
    () => {
      const write = navigator.clipboard?.writeText(command);
      if (write === undefined) {
        select(code);
      } else {
        write.then(
          () => {
            copy.textContent = "Copied";
          },
          () => select(code),
        );
      }
    },
    true,
  );
  const close = (): void => {
    shown.close();
    done();
  };
  const shown = dialog({
    title: "Run it yourself",
    description:
      then === "chooseFolder"
        ? "Run this in a terminal and sign in to Steam there. Then choose the folder it downloaded to."
        : "Run this in a terminal and sign in to Steam there. It downloads the files that need repair. Then verify them.",
    body: [
      get === null
        ? null
        : h(
            "p",
            { class: "command-get" },
            `First get DepotDownloader ${get.version}, unzip it, and run this in its folder: `,
            button.link(get.url, () => void ctx.send({ type: "openUrl", url: get.url })),
          ),
      h("div", { class: "command-line" }, code, copy),
    ],
    actions: [
      button.secondary("Close", close),
      then === "chooseFolder"
        ? button.primary("Choose folder", () => {
            void ctx.send({ type: "pickFolder", purpose: "game" });
            close();
          })
        : button.primary("Verify", () => {
            void ctx.send({ type: "check", level: "quick" });
            close();
          }),
    ],
    onClose: close,
  });
  return { el: shown.el, close };
}

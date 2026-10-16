// An update, ready (R7; the update board): a note in the stage's corner that never takes the
// stage. Restart installs it; Later puts it away, and Settings keeps Restart until then.
import type { Ctx } from "../app";
import { h } from "../dom";
import type { Snapshot } from "../types";
import { button } from "./button";
import { icons } from "./icons";

export function updateToast(ctx: Ctx, s: Snapshot): HTMLElement {
  return h(
    "div",
    { class: "toast", role: "status" },
    h(
      "div",
      { class: "toast-words" },
      h("p", { class: "toast-title" }, "Update ready"),
      h("p", { class: "toast-line" }, `${s.update.available ?? ""} · installs on restart`),
    ),
    button.secondary("Restart", () => void ctx.send({ type: "installUpdate" }), true),
    button.icon("Later", icons.close(), () => void ctx.send({ type: "dismissUpdate" })),
  );
}

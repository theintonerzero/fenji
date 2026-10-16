// A popover (R7): 404 px at the stage's right, under the top bar's controls, over a light scrim on
// the stage. The top bar stays above it with its control shown open. Escape, the scrim or Close
// closes it; focus moves in when it opens and back to its control when it closes.
import { h } from "../dom";
import { button } from "./button";
import { trapFocus } from "./dialog";
import { icons } from "./icons";

export interface Popover {
  el: HTMLElement;
  /** Takes it away, and gives focus back to its control. */
  close(): void;
}

let next = 0;

export function popover(title: string, body: HTMLElement, control: HTMLElement, onClose: () => void): Popover {
  next += 1;
  const titleId = `popover-${next}-title`;
  const panel = h(
    "section",
    { class: "popover", role: "dialog", "aria-modal": "true", "aria-labelledby": titleId, tabindex: "-1" },
    h(
      "div",
      { class: "popover-head" },
      h("h2", { class: "popover-title", id: titleId }, title),
      button.icon("Close", icons.close(), onClose),
    ),
    h("div", { class: "popover-body" }, body),
  );
  const scrim = h("div", { class: "popover-scrim", onmousedown: onClose });
  const el = h("div", { class: "popover-layer" }, scrim, panel);
  trapFocus(panel, onClose);
  queueMicrotask(() => panel.focus());
  return {
    el,
    close() {
      el.remove();
      if (control.isConnected && !control.hidden) {
        control.focus();
      }
    },
  };
}

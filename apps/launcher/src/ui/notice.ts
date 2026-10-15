// The notice (launcher spec §3.12): the last thing that went wrong, or a word from the shell, in
// a card above the screen's button row, with its own close button. An error leads with the
// circled exclamation in danger (The Status Never Alone Rule); a word from the shell, with info.
import { h, type Child } from "../dom";
import type { Notice } from "../types";
import { icons } from "./icons";

export function noticeCard(notice: Notice, onDismiss: () => void, ...more: Child[]): HTMLElement {
  return h(
    "div",
    { class: notice.error ? "notice-card notice-card-error" : "notice-card", role: notice.error ? "alert" : "status" },
    notice.error ? icons.error(16) : icons.info(16),
    h("div", { class: "notice-card-body" }, h("p", {}, notice.text), ...more),
    h(
      "button",
      { class: "notice-card-close", type: "button", "aria-label": "Dismiss", onclick: onDismiss },
      icons.close(),
    ),
  );
}

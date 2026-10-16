// Status (DESIGN.md, Status): the line under a server's name, the host's notice, and meta lines.
// A colour never travels without words, and errors also carry their icon.
import { h, type Child } from "../dom";
import { activeServer } from "../route";
import type { ServerView, Snapshot } from "../types";
import { statusLine as words } from "../words";
import { icons } from "./icons";

/** An 8 px dot by state, or for an error the circled exclamation. A server that doesn't answer is an
 * error on Play and in the column (the Offline board), and a faint dot in a Servers row (`row`, as
 * the Servers board draws it). */
export function statusMark(v: ServerView | undefined, row = false): Element {
  switch (v?.status.kind) {
    case "online":
      return h("span", { class: "dot dot-ok", "aria-hidden": "true" });
    case "maintenance":
      return h("span", { class: "dot dot-warn", "aria-hidden": "true" });
    case "offline":
      return row ? h("span", { class: "dot", "aria-hidden": "true" }) : errorMark();
    case "keyChanged":
    case "ownKey":
    case "incompatible":
      return errorMark();
    default:
      return h("span", { class: "dot", "aria-hidden": "true" });
  }
}

function errorMark(): Element {
  const icon = icons.error(16);
  icon.classList.add("status-icon");
  return icon;
}

/** The active server's status line, which updates itself. */
export function statusLine(s: Snapshot, now: () => number = Date.now): HTMLElement & {
  update(s: Snapshot): void;
} {
  const el = h("p", { class: "status-line", role: "status" });
  const update = (next: Snapshot): void => {
    el.replaceChildren(statusMark(activeServer(next)), words(next, now()));
  };
  update(s);
  return Object.assign(el, { update });
}

/** The host's notice, led by the host's name in gold: "Wen: the lobby is open." */
export function hostNotice(v: ServerView | undefined): HTMLElement | null {
  if (!v || v.motd === "") {
    return null;
  }
  return h("p", { class: "notice prose" }, v.host ? [h("strong", {}, `${v.host}:`), " "] : null, v.motd);
}

/** A meta line: a small icon and the words. */
export function metaLine(icon: Element | null, ...words: Child[]): HTMLElement {
  return h("p", { class: "meta" }, icon, ...words);
}

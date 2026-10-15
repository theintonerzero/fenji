// What the screens share (DESIGN.md, Layout and The context column): parts that renew only when
// what they show changes, the server's facts, the column's sections, the notice's place, and a
// form's own error line.
import type { Ctx } from "../app";
import { h, type Child } from "../dom";
import type { ServerView, Snapshot } from "../types";
import { facts } from "../ui/card";
import { cardSection, column, section } from "../ui/column";
import { icons } from "../ui/icons";
import { noticeCard } from "../ui/notice";
import { statusMark } from "../ui/status";
import { backAround, seen } from "../words";

/** The account service's rules (M3), in the model's words for them (crates/fenji-launcher, app/words.rs). */
export const USERNAME = /^[A-Za-z0-9_-]{3,16}$/;
export const USERNAME_RULE = "Usernames are 3 to 16 letters, digits, _ or -.";
export const PASSWORD_RULE = "Passwords are 10 to 128 characters.";

const shown = new WeakMap<Element, string>();

/**
 * Rebuilds `el`'s children only when `key` changes. Snapshots come often (a check's progress
 * comes several times a second), so a live region speaks once per change, and a button inside
 * keeps the player's focus.
 */
export function renew(el: Element, key: string, build: () => Child[]): void {
  if (shown.get(el) === key) {
    return;
  }
  shown.set(el, key);
  el.replaceChildren(...h("div", {}, ...build()).childNodes);
}

/** A line led by a small icon: the assurances on Welcome, the notes under a form. */
export function iconLine(icon: Element, ...words: Child[]): HTMLElement {
  return h("p", { class: "icon-line" }, icon, h("span", {}, ...words));
}

/** The facts a screen's Server card can show, in the order the screen names them. */
export type Fact = "status" | "back" | "lastSeen" | "address" | "host" | "version" | "players";

/** The status in a word or two, beside its dot or icon. */
function statusWord(v: ServerView): string {
  switch (v.status.kind) {
    case "online":
      return "Online";
    case "maintenance":
      return "Maintenance";
    case "offline":
      return "Not responding";
    case "keyChanged":
      return "Key changed";
    case "ownKey":
      return "Own key, not trusted yet";
    case "incompatible":
      return v.status.newer ? "Newer Fenji" : "Another game build";
    case "unknown":
      return "Checking…";
  }
}

export function serverCard(v: ServerView, rows: Fact[], now: number): HTMLElement {
  const pairs: [string, Child][] = [];
  for (const row of rows) {
    switch (row) {
      case "status":
        pairs.push(["Status", h("span", { class: "fact-status" }, statusMark(v), statusWord(v))]);
        break;
      case "back":
        if (v.status.kind === "maintenance" && v.status.until !== null) {
          pairs.push(["Back", `around ${backAround(v.status.until, now)}`]);
        }
        break;
      case "lastSeen":
        if (v.lastSeenMs !== null) {
          pairs.push(["Last seen", seen(v.lastSeenMs, now)]);
        }
        break;
      case "address":
        pairs.push(["Address", v.address]);
        break;
      case "host":
        if (v.host !== null) {
          pairs.push(["Host", v.host]);
        }
        break;
      case "version":
        pairs.push(["Version", v.version === null ? "Not known yet" : `Fenji ${v.version}`]);
        break;
      case "players":
        pairs.push(["Players", v.players === null ? "Not known yet" : `${v.players.online} of ${v.players.max}`]);
        break;
    }
  }
  return cardSection("Server", facts(pairs));
}

/** A column section of plain paragraphs. */
export function textSection(heading: string, ...paragraphs: Child[]): HTMLElement {
  return section(heading, paragraphs.map((p) => h("p", { class: "column-text" }, p)));
}

/** The host's notice in the column, led by the host's name in gold; none without one. */
export function hostNoticeSection(v: ServerView): HTMLElement | null {
  if (v.motd === "") {
    return null;
  }
  return section(
    "Host notice",
    h("p", { class: "column-text notice" }, v.host ? [h("strong", {}, `${v.host}:`), " "] : null, v.motd),
  );
}

/** The context column, rebuilt only when `key` (what its sections show) changes. */
export function contextColumn(): { el: HTMLElement; show(key: string, sections: () => Child[], foot: string): void } {
  const el = column([]);
  return {
    el,
    show(key, sections, foot) {
      renew(el, `${key}\n${foot}`, () => [...sections(), h("p", { class: "column-foot" }, foot)]);
    },
  };
}

export interface NoticeSlot {
  el: HTMLElement;
  /** Shows the snapshot's notice, with `more` (a button) under its words. */
  update(s: Snapshot, more?: Child[]): void;
  /** While a form's send waits, its refusal can reach the snapshot first: hold the notice still. */
  hold(on: boolean): void;
  /** A refusal the form shows by its field, which the notice doesn't repeat. */
  inline(text: string): void;
}

/** Where the snapshot's notice shows (launcher spec §3.12), with a close button (DismissNotice). */
export function noticeSlot(ctx: Ctx): NoticeSlot {
  const el = h("div", { class: "notice-slot" });
  let last: Snapshot | null = null;
  let extra: Child[] = [];
  let held = false;
  let inline: string | null = null;
  const render = (): void => {
    if (held || last === null) {
      return;
    }
    const notice = last.notice !== null && last.notice.text !== inline ? last.notice : null;
    const key = notice === null ? "" : `${notice.error}\n${notice.text}\n${extra.length}`;
    renew(el, key, () =>
      notice === null ? [] : [noticeCard(notice, () => void ctx.send({ type: "dismissNotice" }), ...extra)],
    );
  };
  return {
    el,
    update(s, more = []) {
      last = s;
      extra = more;
      render();
    },
    hold(on) {
      held = on;
      render();
    },
    inline(text) {
      inline = text;
      render();
    },
  };
}

/**
 * Shows a refusal where it belongs and keeps it from waiting as the notice too: the model makes
 * every refusal the snapshot's notice, so a screen that shows one by its field dismisses it.
 */
export function refused(ctx: Ctx, slot: NoticeSlot, text: string, show: (text: string) => void): void {
  slot.inline(text);
  show(text);
  void ctx.send({ type: "dismissNotice" });
}

/** A form's own error line, for a refusal that concerns no one field. */
export function formError(): { el: HTMLElement; set(text: string | null): void } {
  const words = h("span", {});
  const el = h("p", { class: "field-error form-error", role: "alert", hidden: true }, icons.error(14), words);
  return {
    el,
    set(text) {
      words.textContent = text ?? "";
      el.hidden = text === null;
    },
  };
}

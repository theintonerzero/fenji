// What the screens and popovers share: parts that renew only when what they show changes, the
// model's notice as one line, a form's own error line, and paths as a player knows them.
import type { Ctx } from "../app";
import { h, type Child } from "../dom";
import type { Snapshot } from "../types";
import { button } from "../ui/button";
import { icons } from "../ui/icons";

/** The account service's rules (M3), in the model's words for them (crates/fenji-launcher, app/words.rs). */
export const USERNAME = /^[A-Za-z0-9_-]{3,16}$/;
export const USERNAME_RULE = "Usernames are 3 to 16 letters, digits, _ or -.";
export const PASSWORD_RULE = "Passwords are 10 to 128 characters.";

const shownKeys = new WeakMap<Element, string>();

/**
 * Rebuilds `el`'s children only when `key` changes. Snapshots come often (a check's progress
 * comes several times a second), so a live region speaks once per change, and a button inside
 * keeps the player's focus.
 */
export function renew(el: Element, key: string, build: () => Child[]): void {
  if (shownKeys.get(el) === key) {
    return;
  }
  shownKeys.set(el, key);
  el.replaceChildren(...h("div", {}, ...build()).childNodes);
}

/** A path as the player knows it, the home folder as `~`, so a screenshot doesn't carry the
 * player's name. For display only: intents carry the whole path. */
export function shown(text: string, home: string | null): string {
  if (home === null || home === "" || !(text === home || text.startsWith(`${home}/`))) {
    return text;
  }
  return `~${text.slice(home.length)}`;
}

/** A path's last folders ("~/…/Swords of Legends Online") where a narrow place would cut it, the
 * whole in its title. */
export function pathEnd(whole: string, max: number, home: string | null = null): HTMLElement {
  const text = shown(whole, home);
  if (text.length <= max) {
    return h("span", { class: "mono path" }, text);
  }
  const parts = text.split(/(?=[/\\])/);
  let kept = parts[parts.length - 1] ?? text;
  for (let i = parts.length - 2; i >= 1 && parts[i]!.length + kept.length + 3 <= max; i -= 1) {
    kept = parts[i] + kept;
  }
  const head = parts[0] ?? "";
  return h("span", { class: "mono path", title: text }, `${head}/…${kept}`);
}

export interface NoticeSlot {
  el: HTMLElement;
  /** Shows the snapshot's notice, with `more` (a link) after its words. */
  update(s: Snapshot, more?: Child[]): void;
  /** While a form's send waits, its refusal can reach the snapshot first: hold the notice still. */
  hold(on: boolean): void;
  /** A refusal the form shows by its field, which the notice doesn't repeat. */
  inline(text: string): void;
}

/**
 * The model's notice (launcher spec §3.12) as one line: an error leads with the circled
 * exclamation, a word from the shell with the info mark, and Dismiss puts it away (DismissNotice).
 */
export function noticeSlot(ctx: Ctx): NoticeSlot {
  const el = h("div", { class: "note-slot" });
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
      notice === null
        ? []
        : [
            h(
              "div",
              { class: notice.error ? "note note-error" : "note", role: notice.error ? "alert" : "status" },
              notice.error ? icons.error(18) : icons.info(18),
              h("p", { class: "note-words" }, notice.text, ...extra.map((more) => [" ", more])),
              button.icon("Dismiss", icons.close(), () => void ctx.send({ type: "dismissNotice", text: notice.text })),
            ),
          ],
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
 * every refusal the snapshot's notice, so a form that shows one by its field dismisses it.
 */
export function refused(ctx: Ctx, slot: NoticeSlot | null, text: string, show: (text: string) => void): void {
  slot?.inline(text);
  show(text);
  void ctx.send({ type: "dismissNotice", text });
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

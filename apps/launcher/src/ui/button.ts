// Buttons (the redesign's boards). PLAY kindles (R3) and is the only tracked uppercase label; it and
// a task's one primary action are the only ember a screen shows. Secondary actions are outlines,
// places to go are gold words, and a quiet action is muted words.
import { h, type Child } from "../dom";
import type { PlayLook } from "../route";
import { playWords } from "../words";

type OnClick = (event: Event) => void;

function reducedMotion(): boolean {
  return typeof window.matchMedia === "function" && window.matchMedia("(prefers-reduced-motion: reduce)").matches;
}

/**
 * Sets PLAY's look in place, so the stage keeps one button across snapshots and a player's focus
 * stays on it. Kindling, it fills to `--p` with ember; with no progress to show, the fill drifts.
 */
export function setPlay(el: HTMLButtonElement, look: PlayLook): void {
  // Kindling to lit: the fill completes and dissolves into the ember (the redesign's finish review),
  // unless motion is reduced.
  if (el.dataset.look === "kindle" && look.kind === "lit" && !reducedMotion()) {
    el.classList.add("kindled");
    el.addEventListener("animationend", () => el.classList.remove("kindled"), { once: true });
  } else if (look.kind !== "lit") {
    el.classList.remove("kindled");
  }
  el.dataset.look = look.kind;
  el.classList.toggle("ember", look.kind === "lit");
  el.disabled = look.kind !== "lit";
  const percent = look.kind === "kindle" ? look.percent : null;
  if (percent === null) {
    el.style.removeProperty("--p");
  } else {
    el.style.setProperty("--p", `${Math.max(0, Math.min(100, percent))}%`);
  }
  el.classList.toggle("drift", look.kind === "kindle" && percent === null);
  const words = playWords(look);
  const label = el.firstElementChild as HTMLElement | null;
  if (label?.dataset.words === words) {
    return;
  }
  const next = h("span", { class: "play-label", "data-words": words });
  if (look.kind === "running") {
    next.append(h("span", { class: "dot", "aria-hidden": "true" }));
  }
  next.append(words);
  el.replaceChildren(next);
}

export const button = {
  /** PLAY, found by its `play` class. */
  play(onclick: OnClick): HTMLButtonElement {
    const el = h("button", { class: "play", type: "button", onclick });
    setPlay(el, { kind: "closed" });
    return el;
  },

  /** A task's one action when it isn't Play: Continue, Create account, Repair. */
  primary(label: Child, onclick?: OnClick, small = false): HTMLButtonElement {
    return h("button", { class: small ? "btn btn-sm ember" : "btn ember", type: "button", onclick }, label);
  },

  /** An outline: Cancel, Refresh, Retry, Verify integrity. */
  secondary(label: Child, onclick?: OnClick, small = false): HTMLButtonElement {
    return h("button", { class: small ? "btn btn-sm" : "btn", type: "button", onclick }, label);
  },

  /** Gold words: a place to go, never the primary action. */
  link(label: Child, onclick?: OnClick): HTMLButtonElement {
    return h("button", { class: "link", type: "button", onclick }, label);
  },

  /** Muted words beside PLAY or a primary action: Stop. */
  quiet(label: Child, onclick?: OnClick): HTMLButtonElement {
    return h("button", { class: "quiet", type: "button", onclick }, label);
  },

  /** A control drawn as an icon alone, named for assistive technology. */
  icon(label: string, icon: Element, onclick?: OnClick): HTMLButtonElement {
    return h("button", { class: "icon-button", type: "button", "aria-label": label, onclick }, icon);
  },
};

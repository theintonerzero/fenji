// Buttons (DESIGN.md, Buttons). Only PLAY and the worded primary carry the ember class: one per
// screen (The One Ember Rule). PLAY is the only tracked uppercase label (The Tracked Caps Rule).
import { h, type Child } from "../dom";

export type PlayState =
  | { kind: "ready" }
  /** Visible but closed; the screen writes the reason beside it. */
  | { kind: "disabled" }
  /** Closed while the game starts: it reads "Starting". */
  | { kind: "starting" }
  | { kind: "running" };

type OnClick = (event: Event) => void;

/**
 * Sets PLAY's state in place, so a screen keeps one button across snapshots and a player's focus
 * stays on it (plan LP4).
 */
export function setPlay(el: HTMLButtonElement, state: PlayState): void {
  el.classList.toggle("ember", state.kind === "ready");
  el.disabled = state.kind !== "ready";
  switch (state.kind) {
    case "ready":
    case "disabled":
      el.replaceChildren("Play");
      break;
    case "starting":
      el.replaceChildren("Starting");
      break;
    case "running":
      el.replaceChildren(h("span", { class: "dot dot-ok", "aria-hidden": "true" }), "Running");
      break;
  }
}

export const button = {
  /** PLAY, found by its `play` class. */
  play(state: PlayState, onclick?: OnClick): HTMLButtonElement {
    const el = h("button", { class: "btn btn-play play", type: "button", onclick });
    setPlay(el, state);
    return el;
  },

  /** The screen's one action when it isn't Play: Continue, Create account, Repair from Steam. */
  primary(label: string, onclick?: OnClick): HTMLButtonElement {
    return h("button", { class: "btn btn-primary ember", type: "button", onclick }, label);
  },

  secondary(
    label: Child,
    onclick?: OnClick,
    size: "regular" | "compact" | "small" = "regular",
  ): HTMLButtonElement {
    const sized = size === "regular" ? "" : ` btn-${size}`;
    return h("button", { class: `btn${sized}`, type: "button", onclick }, label);
  },

  /** A gold text button: a place to go, never the primary action (The Gold Is People Rule). */
  link(label: Child, onclick?: OnClick): HTMLButtonElement {
    return h("button", { class: "btn btn-link", type: "button", onclick }, label);
  },
};

// Buttons (DESIGN.md, Buttons). Only PLAY and the worded primary carry the ember class: one per
// screen (The One Ember Rule). PLAY is the only tracked uppercase label (The Tracked Caps Rule).
import { h, type Child } from "../dom";

export type PlayState =
  | { kind: "ready" }
  /** Visible but closed; the screen writes the reason beside it. */
  | { kind: "disabled" }
  | { kind: "running" };

type OnClick = (event: Event) => void;

export const button = {
  play(state: PlayState, onclick?: OnClick): HTMLButtonElement {
    switch (state.kind) {
      case "ready":
        return h("button", { class: "btn btn-play ember", type: "button", onclick }, "Play");
      case "disabled":
        return h("button", { class: "btn btn-play", type: "button", disabled: true }, "Play");
      case "running":
        return h(
          "button",
          { class: "btn btn-play", type: "button", disabled: true },
          h("span", { class: "dot dot-ok", "aria-hidden": "true" }),
          "Running",
        );
    }
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

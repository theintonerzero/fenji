// The top bar (R1; the boards): the server's address, faint, at the left; the Game chip, the
// account, Settings and the window controls at the right. Its empty space drags the window. Each
// control opens its popover under it and shows it open; the window has no maximise.
import type { Ctx, PopoverName } from "../app";
import { h } from "../dom";
import { activeServer, type ScreenName } from "../route";
import type { Snapshot } from "../types";
import { gameChip } from "../words";
import { button } from "./button";
import { icons } from "./icons";
import { initials } from "./people";

export interface TopBar {
  el: HTMLElement;
  /** The control a popover belongs to. */
  control(which: PopoverName): HTMLElement;
  /** `modal`: a dialog is open. The top bar stays above its scrim, so the window's controls and
   * its drag keep working, and the popovers' controls wait for it (#376). */
  update(s: Snapshot, screen: ScreenName, open: PopoverName | null, modal?: boolean): void;
}

export function topBar(ctx: Ctx): TopBar {
  const address = h("span", { class: "address", "data-tauri-drag-region": true });
  const chipWords = h("span", {});
  const chip = h(
    "button",
    { class: "chip", type: "button", "aria-haspopup": "dialog", onclick: () => ctx.toggle("game") },
    h("span", { class: "dot", "aria-hidden": "true" }),
    chipWords,
  );
  const me = h("button", { class: "me", type: "button", "aria-haspopup": "dialog", onclick: () => ctx.toggle("account") });
  const gear = button.icon("Settings", icons.gear(18), () => ctx.toggle("settings"));
  gear.setAttribute("aria-haspopup", "dialog");
  const controls: Record<PopoverName, HTMLElement> = { game: chip, account: me, settings: gear };
  const el = h(
    "header",
    { class: "top", "data-tauri-drag-region": true },
    address,
    h("span", { class: "spacer", "data-tauri-drag-region": true }),
    chip,
    me,
    gear,
    h(
      "div",
      { class: "window-controls" },
      button.icon("Minimise", icons.minimise(), () => void ctx.send({ type: "minimize" })),
      button.icon("Close", icons.close(), () => void ctx.send({ type: "close" })),
    ),
  );
  let meKey = "";
  return {
    el,
    control: (which) => controls[which],
    update(s, screen, open, modal = false) {
      const v = activeServer(s);
      address.textContent = screen === "welcome" ? "" : (v?.address ?? "");
      // The boards: no chip until there's a server and an account to play with; the account on the
      // stage only, where its popover means something.
      chip.hidden = screen === "welcome" || screen === "redeem";
      me.hidden = screen !== "stage";
      const { label, tone } = gameChip(s);
      if (chipWords.textContent !== label) {
        chipWords.textContent = label;
      }
      chip.dataset.tone = tone;
      chip.setAttribute("aria-label", `Game: ${label}`);
      const name = v?.signedIn ? v.username : null;
      const key = name ?? "";
      if (key !== meKey || me.childElementCount === 0) {
        meKey = key;
        me.replaceChildren(name === null ? icons.person(16) : h("span", { "aria-hidden": "true" }, initials(name)));
        me.setAttribute("aria-label", name === null ? "Account" : `Account: ${name}`);
      }
      for (const [which, control] of Object.entries(controls) as [PopoverName, HTMLElement][]) {
        control.classList.toggle("open", open === which);
        control.setAttribute("aria-expanded", String(open === which));
        control.toggleAttribute("inert", modal);
      }
    },
  };
}

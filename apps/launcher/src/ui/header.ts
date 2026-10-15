// The header (DESIGN.md, Header and tabs; the boards): the seal and FENJI, the four tabs (the
// boards' "Server" is "Servers", L7), then the account pill, Settings, Minimise and Close. There
// is no maximise: the window is fixed. Its empty space drags the window.
import type { Ctx } from "../app";
import { h } from "../dom";
import { activeServer, route, type ScreenName, type Tab } from "../route";
import type { Snapshot } from "../types";
import { icons } from "./icons";
import { avatar } from "./people";

const SIGN_IN_SCREENS = new Set<ScreenName>(["welcome", "redeem", "signIn"]);

const TABS: [Tab, string][] = [
  ["play", "Play"],
  ["servers", "Servers"],
  ["account", "Account"],
  ["client", "Client"],
];

export function header(ctx: Ctx, s: Snapshot): HTMLElement & { update(s: Snapshot): void } {
  const tabs = TABS.map(([tab, label]) =>
    h("button", { class: "tab", type: "button", onclick: () => ctx.setTab(tab) }, label),
  );
  const pill = h("button", { class: "pill", type: "button", onclick: () => ctx.setTab("account") });
  const settings = h(
    "button",
    { class: "window-control", type: "button", "aria-label": "Settings", onclick: () => ctx.setTab("settings") },
    icons.settings(16),
  );
  const el = h(
    "header",
    { class: "header", "data-tauri-drag-region": true },
    h(
      "div",
      { class: "brand", "data-tauri-drag-region": true },
      h("span", { class: "seal", "aria-hidden": "true" }, "焚"),
      h("span", { class: "wordmark", "aria-label": "Fenji" }, "FENJI"),
    ),
    h("nav", { class: "tabs", "aria-label": "Main" }, tabs),
    h("div", { class: "drag", "data-tauri-drag-region": true }),
    h(
      "div",
      { class: "header-end" },
      pill,
      settings,
      h(
        "button",
        {
          class: "window-control",
          type: "button",
          "aria-label": "Minimise",
          onclick: () => void ctx.send({ type: "minimize" }),
        },
        icons.minimise(),
      ),
      h(
        "button",
        { class: "window-control", type: "button", "aria-label": "Close", onclick: () => void ctx.send({ type: "close" }) },
        icons.close(),
      ),
    ),
  );
  const update = (next: Snapshot): void => {
    TABS.forEach(([tab], i) => {
      if (tab === ctx.tab) {
        tabs[i]!.setAttribute("aria-current", "page");
      } else {
        tabs[i]!.removeAttribute("aria-current");
      }
    });
    if (ctx.tab === "settings") {
      settings.setAttribute("aria-current", "page");
    } else {
      settings.removeAttribute("aria-current");
    }
    const server = activeServer(next);
    const name = server?.signedIn ? server.username : null;
    // Signed out, the pill offers Sign in, except on the screens that are the sign-in already
    // (the boards show none there).
    pill.hidden = name === null && SIGN_IN_SCREENS.has(route(ctx.tab, next));
    pill.classList.toggle("pill-signed-out", name === null);
    pill.replaceChildren(...(name === null ? ["Sign in"] : [avatar(name, null, "small"), name]));
  };
  update(s);
  return Object.assign(el, { update });
}

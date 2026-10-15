// The view's frame (launcher spec §3.1, §3.2; plan LP4): the header, and the screen the route
// names. A screen mounts once per route and updates in place on each snapshot, so a form keeps
// its input and focus while the state changes around it. A dialog comes over any screen while the
// model asks for one: a link's other key first, then a downloaded update.
import { h } from "./dom";
import { route, type ScreenName, type Tab } from "./route";
import { account } from "./screens/account";
import { clientReady } from "./screens/client-ready";
import { clientRepair } from "./screens/client-repair";
import { clientSetup } from "./screens/client-setup";
import { clientWorking } from "./screens/client-working";
import { play } from "./screens/play";
import { redeem } from "./screens/redeem";
import { servers } from "./screens/servers";
import { settings } from "./screens/settings";
import { signIn } from "./screens/sign-in";
import { welcome } from "./screens/welcome";
import type { Intent, Snapshot } from "./types";
import { header } from "./ui/header";
import { keyDialog } from "./ui/key-dialog";
import { updateDialog } from "./ui/update-dialog";

export interface Ctx {
  send(intent: Intent): Promise<string | null>;
  setTab(tab: Tab): void;
  tab: Tab;
}

export interface Screen {
  update(s: Snapshot): void;
  destroy(): void;
}

export type ScreenFactory = (root: HTMLElement, ctx: Ctx) => Screen;

/** The screens by name: every one the route can name. */
export const screens: Record<ScreenName, ScreenFactory> = {
  welcome,
  redeem,
  signIn,
  account,
  play,
  servers,
  clientSetup,
  clientWorking,
  clientRepair,
  clientReady,
  settings,
};

type Overlay = "keyChange" | "update";

/** The dialog the snapshot asks for, if any: a link's other key before an update. */
function overlayFor(s: Snapshot): Overlay | null {
  if (s.keyChange !== null) {
    return "keyChange";
  }
  return s.update.showDialog ? "update" : null;
}

const OVERLAYS: Record<Overlay, (ctx: Ctx, s: Snapshot) => HTMLElement & { close(): void }> = {
  keyChange: keyDialog,
  update: updateDialog,
};

/** The shell, as the view sees it (ipc.ts; dev-golden.ts in development). */
export interface Ipc {
  snapshot(): Promise<Snapshot>;
  onSnapshot(cb: (s: Snapshot) => void): Promise<() => void>;
  send(intent: Intent): Promise<string | null>;
}

/** Mounts the view in `root`, starting on `start`, and keeps it on the model's snapshots. */
export async function mount(root: HTMLElement, ipc: Ipc, start: Tab = "play"): Promise<void> {
  let snapshot = await ipc.snapshot();
  const ctx: Ctx = {
    tab: start,
    send: (intent) => ipc.send(intent),
    setTab(tab) {
      if (tab === ctx.tab) {
        return;
      }
      ctx.tab = tab;
      void ipc.send({ type: "viewTab", tab });
      render();
    },
  };
  const head = header(ctx, snapshot);
  const body = h("div", { class: "screen" });
  const overlay = h("div", {});
  root.replaceChildren(h("div", { class: "window" }, head, body), overlay);
  let current: { name: ScreenName; screen: Screen } | null = null;
  let dialog: { which: Overlay; key: string; el: HTMLElement & { close(): void } } | null = null;

  function render(): void {
    head.update(snapshot);
    const name = route(ctx.tab, snapshot);
    if (current?.name !== name) {
      current?.screen.destroy();
      body.replaceChildren();
      current = { name, screen: screens[name](body, ctx) };
    }
    current.screen.update(snapshot);
    // A dialog is drawn once for what it asks, and again only when that changes.
    const which = overlayFor(snapshot);
    const key = which === "keyChange" ? JSON.stringify(snapshot.keyChange) : JSON.stringify(snapshot.update);
    if (dialog !== null && (dialog.which !== which || dialog.key !== key)) {
      dialog.el.close();
      dialog = null;
    }
    if (which !== null && dialog === null) {
      dialog = { which, key, el: OVERLAYS[which](ctx, snapshot) };
      overlay.append(dialog.el);
    }
  }

  render();
  void ipc.send({ type: "viewTab", tab: ctx.tab });
  window.addEventListener("focus", () => void ipc.send({ type: "focus", focused: true }));
  window.addEventListener("blur", () => void ipc.send({ type: "focus", focused: false }));
  await ipc.onSnapshot((next) => {
    snapshot = next;
    render();
  });
}

// The view's frame (launcher spec §3.1, §3.2; plan LP4): the header, and the screen the route
// names. A screen mounts once per route and updates in place on each snapshot, so a form keeps
// its input and focus while the state changes around it. The update dialog comes over any screen
// while the model asks for it.
import { h } from "./dom";
import { activeServer, bladeFor, groundFor, route, type ScreenName, type Tab } from "./route";
import { account } from "./screens/account";
import { play } from "./screens/play";
import { redeem } from "./screens/redeem";
import { signIn } from "./screens/sign-in";
import { welcome } from "./screens/welcome";
import type { Intent, Snapshot } from "./types";
import { blade } from "./ui/blade";
import { facts } from "./ui/card";
import { cardSection, column } from "./ui/column";
import { header } from "./ui/header";
import { people } from "./ui/people";
import { hostNotice, statusLine } from "./ui/status";
import { versionLine } from "./words";

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

/** The screens by name. A screen that isn't here yet gets a placeholder with its name. */
export const screens: Partial<Record<ScreenName, ScreenFactory>> = { welcome, redeem, signIn, account, play };

/** What comes over any screen: the update dialog, while `update.showDialog` (launcher Task 14). */
export const overlays: { update?: ScreenFactory } = {};

/** The shell, as the view sees it (ipc.ts; dev-golden.ts in development). */
export interface Ipc {
  snapshot(): Promise<Snapshot>;
  onSnapshot(cb: (s: Snapshot) => void): Promise<() => void>;
  send(intent: Intent): Promise<string | null>;
}

const TITLES: Record<ScreenName, string> = {
  welcome: "Welcome",
  redeem: "Create your account",
  signIn: "Sign in",
  account: "Your account",
  play: "Play",
  servers: "Servers",
  clientSetup: "Set up the game",
  clientWorking: "Downloading",
  clientRepair: "Repair",
  clientReady: "Game client",
  settings: "Settings",
};

/** The screens whose subject is readiness, where the blade stands (The Readiness Only Rule). */
const HERO = new Set<ScreenName>(["welcome", "play", "clientSetup", "clientWorking", "clientRepair", "clientReady"]);

/** Until a screen is built: its name, and the parts of the frame it will stand in. */
function placeholder(name: ScreenName): ScreenFactory {
  return (root) => {
    const main = h("main", { class: "main" });
    const art = HERO.has(name) ? blade({ kind: "unlit" }) : null;
    const text = h("div", { class: "placeholder" });
    let aside: HTMLElement = column([]);
    if (art) {
      main.append(art);
    }
    main.append(text);
    root.append(main, aside);
    return {
      update(s) {
        art?.update(bladeFor(s), groundFor(s));
        const server = activeServer(s);
        const lines = [
          h("h1", { class: "display" }, server?.name ?? TITLES[name]),
          server ? statusLine(s) : null,
          hostNotice(server),
          h("p", { class: "placeholder-name" }, `${TITLES[name]}: this screen comes with the next part of the launcher.`),
        ];
        text.replaceChildren(...lines.filter((line): line is HTMLElement => line !== null));
        const next = column(
          [
            people(server?.signedIn ? s.lobby : null, server?.username ?? null, server?.host ?? null),
            server
              ? cardSection(
                  "Server",
                  facts([
                    ["Address", server.address],
                    ["Version", server.version === null ? "Not known yet" : `Fenji ${server.version}`],
                    ["Players", server.players ? `${server.players.online} of ${server.players.max}` : "Not known yet"],
                  ]),
                )
              : null,
          ],
          versionLine(s),
        );
        aside.replaceWith(next);
        aside = next;
      },
      destroy() {
        root.replaceChildren();
      },
    };
  };
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
  let dialog: Screen | null = null;

  function render(): void {
    head.update(snapshot);
    const name = route(ctx.tab, snapshot);
    if (current?.name !== name) {
      current?.screen.destroy();
      body.replaceChildren();
      current = { name, screen: (screens[name] ?? placeholder(name))(body, ctx) };
    }
    current.screen.update(snapshot);
    if (snapshot.update.showDialog && overlays.update) {
      dialog ??= overlays.update(overlay, ctx);
      dialog.update(snapshot);
    } else if (dialog) {
      dialog.destroy();
      dialog = null;
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

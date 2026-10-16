// The view's frame (the redesign spec R1): the rail of servers and the stage, whose top bar holds
// the Game chip, the account and Settings. The stage shows the active server, or the one task in its
// place (route.ts). A screen mounts once per route and updates in place on each snapshot, so a form
// keeps its input and focus while the state changes around it. Popovers and the rail's dialogs are
// the view's own state; a changed key's dialog and an update's toast come when the model asks.
import { h } from "./dom";
import { accountPopover } from "./popovers/account";
import { gamePopover } from "./popovers/game";
import { settingsPopover } from "./popovers/settings";
import { groundFor, route, type ScreenName, type ViewState } from "./route";
import { redeem } from "./screens/redeem";
import { signIn } from "./screens/sign-in";
import { stage } from "./screens/stage";
import { welcome } from "./screens/welcome";
import type { Intent, ServerView, Snapshot } from "./types";
import type { Dialog } from "./ui/dialog";
import { keyDialog } from "./ui/key-dialog";
import { popover, type Popover } from "./ui/popover";
import { rail } from "./ui/rail";
import { addDialog, removeDialog } from "./ui/server-dialogs";
import { updateToast } from "./ui/toast";
import { topBar } from "./ui/topbar";

export type PopoverName = "game" | "account" | "settings";

export interface Ctx {
  send(intent: Intent): Promise<string | null>;
  /** Opens a popover, closing any other; `null` closes the one that's open. */
  open(which: PopoverName | null): void;
  /** A top bar control: opens its popover, or closes it when it's the one open. */
  toggle(which: PopoverName): void;
  /** Chooses a server in the rail. */
  select(id: number): void;
  /** Shows sign in for a server, or takes it away (ViewState.signIn). */
  signIn(server: number | "added" | null): void;
  /** The Add a server dialog. */
  addServer(): void;
  /** Remove's confirmation for a server. */
  removeServer(v: ServerView): void;
  /** A dialog over the stage, until it calls `done`. */
  show(make: (done: () => void) => Dialog): void;
}

export interface Screen {
  update(s: Snapshot): void;
  destroy(): void;
}

export type ScreenFactory = (root: HTMLElement, ctx: Ctx) => Screen;

/** A popover's title and body, which update in place on each snapshot. */
export interface PopoverContent {
  title: string;
  body: HTMLElement;
  update(s: Snapshot): void;
}

/** The screens by name: every one the route can name. */
export const screens: Record<ScreenName, ScreenFactory> = { welcome, redeem, signIn, stage };

const POPOVERS: Record<PopoverName, (ctx: Ctx) => PopoverContent> = {
  game: gamePopover,
  account: accountPopover,
  settings: settingsPopover,
};

/** Where each popover's control shows (the boards): Game beside a server, Account on the stage. */
const OPENS_ON: Record<PopoverName, ReadonlySet<ScreenName>> = {
  game: new Set(["signIn", "stage"]),
  account: new Set(["stage"]),
  settings: new Set(["welcome", "redeem", "signIn", "stage"]),
};

/** The shell, as the view sees it (ipc.ts; dev-golden.ts in development). */
export interface Ipc {
  snapshot(): Promise<Snapshot>;
  onSnapshot(cb: (s: Snapshot) => void): Promise<() => void>;
  send(intent: Intent): Promise<string | null>;
}

export interface MountOptions {
  /** A popover open from the start: the finish review's screenshots (dev-golden.ts). */
  open?: PopoverName;
  /** Add a server open from the start, for the same. */
  adding?: boolean;
}

/** Mounts the view in `root`, and keeps it on the model's snapshots. */
export async function mount(root: HTMLElement, ipc: Ipc, options: MountOptions = {}): Promise<void> {
  // The view listens before it asks for the first snapshot, so that a change the model makes
  // meanwhile isn't lost until the next one, up to a probe later (#348). One that comes while the
  // first is fetched may be older than the answer, or newer: the view asks once more then.
  let mounted = false;
  let changed = false;
  await ipc.onSnapshot((next) => {
    changed = true;
    if (mounted) {
      snapshot = next;
      render();
    }
  });
  let snapshot = await ipc.snapshot();
  if (changed) {
    snapshot = await ipc.snapshot();
  }

  const view: ViewState = { signIn: null, playFor: null };
  // Play's sign-in was cancelled for the server the player left (see render).
  let cancelledFor: number | null = null;
  let open: { name: PopoverName; content: PopoverContent; shown: Popover } | null = null;
  let shownDialog: Dialog | null = null;
  let adding = false;

  const ctx: Ctx = {
    send: (intent) => ipc.send(intent),
    open: (which) => setPopover(which),
    toggle: (which) => setPopover(open?.name === which ? null : which),
    select(id) {
      view.signIn = null;
      void ipc.send({ type: "selectServer", id });
      render();
    },
    signIn(server) {
      // Welcome's server is known once a snapshot names it, which may already have come.
      view.signIn = server === "added" && snapshot.active !== null ? snapshot.active : server;
      render();
    },
    addServer() {
      adding = true;
      ctx.show((done) =>
        addDialog(ctx, () => {
          adding = false;
          done();
        }),
      );
    },
    removeServer(v) {
      ctx.show((done) => removeDialog(ctx, v, done));
    },
    show(make) {
      setPopover(null);
      shownDialog?.close();
      const made: Dialog = make(() => {
        if (shownDialog === made) {
          shownDialog = null;
        }
        render();
      });
      shownDialog = made;
      layer.append(made.el);
      render();
    },
  };

  const railPart = rail(ctx);
  const top = topBar(ctx);
  const body = h("main", { class: "stage-body" });
  const layer = h("div", { class: "layer" });
  const toastSlot = h("div", { class: "toast-slot" });
  const stageEl = h("div", { class: "stage" }, top.el, body, layer, toastSlot);
  root.replaceChildren(h("div", { class: "window" }, railPart.el, stageEl));
  let current: { name: ScreenName; server: number | null; screen: Screen } | null = null;
  let keyShown: { key: string; el: HTMLElement & { close(): void } } | null = null;
  let toastKey = "";

  function setPopover(which: PopoverName | null): void {
    if ((open?.name ?? null) === which) {
      return;
    }
    if (open !== null) {
      const was = open;
      open = null;
      was.shown.close();
    }
    if (which !== null && OPENS_ON[which].has(route(snapshot, view))) {
      const content = POPOVERS[which](ctx);
      content.update(snapshot);
      const shown = popover(content.title, content.body, top.control(which), () => setPopover(null));
      open = { name: which, content, shown };
      layer.append(shown.el);
    }
    if (mounted) {
      render();
    }
  }

  /** Ties the view's sign-ins to their servers: Welcome's, once it's named; one for a server that's
   * gone, dropped; and Play's, to the server PLAY was pressed on. Leaving that server cancels it, so
   * a password typed for one host never goes to another, and no game starts unasked. */
  function bindSignIns(): void {
    if (view.signIn === "added" && snapshot.active !== null) {
      view.signIn = snapshot.active;
    }
    if (typeof view.signIn === "number" && !snapshot.servers.some((v) => v.id === view.signIn)) {
      view.signIn = null;
    }
    if (!snapshot.needSignIn) {
      view.playFor = null;
      cancelledFor = null;
      return;
    }
    if (view.playFor === null) {
      view.playFor = snapshot.active;
    } else if (snapshot.active !== view.playFor && cancelledFor !== view.playFor) {
      cancelledFor = view.playFor;
      void ipc.send({ type: "cancelSignIn" });
    }
  }

  function render(): void {
    bindSignIns();
    const name = route(snapshot, view);
    stageEl.dataset.ground = groundFor(snapshot, name);
    railPart.update(snapshot, adding);
    // A task screen is drawn for one server: another server's gets a fresh form, never the
    // username and password typed for the first.
    const forServer = name === "stage" ? null : snapshot.active;
    if (current?.name !== name || current.server !== forServer) {
      current?.screen.destroy();
      body.replaceChildren();
      body.dataset.screen = name;
      current = { name, server: forServer, screen: screens[name](body, ctx) };
    }
    current.screen.update(snapshot);
    // A popover whose control the screen no longer shows goes with it.
    if (open !== null && !OPENS_ON[open.name].has(name)) {
      const was = open;
      open = null;
      was.shown.close();
    }
    open?.content.update(snapshot);
    top.update(snapshot, name, open?.name ?? null);
    // A changed key's dialog is drawn once for what it asks, and again only when that changes.
    const key = snapshot.keyChange === null ? "" : JSON.stringify(snapshot.keyChange);
    if (keyShown !== null && keyShown.key !== key) {
      keyShown.el.close();
      keyShown = null;
    }
    if (key !== "" && keyShown === null) {
      setPopover(null);
      keyShown = { key, el: keyDialog(ctx, snapshot) };
      layer.append(keyShown.el);
    }
    const toast = snapshot.update.showDialog ? `${snapshot.update.available}` : "";
    if (toast !== toastKey) {
      toastKey = toast;
      toastSlot.replaceChildren(...(toast === "" ? [] : [updateToast(ctx, snapshot)]));
    }
  }

  render();
  mounted = true;
  if (options.open !== undefined) {
    setPopover(options.open);
  }
  if (options.adding) {
    ctx.addServer();
  }
  window.addEventListener("focus", () => void ipc.send({ type: "focus", focused: true }));
  window.addEventListener("blur", () => void ipc.send({ type: "focus", focused: false }));
}

// Which screen a snapshot shows (the redesign spec §3), and the stage's states: PLAY's look (R3)
// and the ground behind it. The words for them are in words.ts.
import type { ServerView, Snapshot } from "./types";

/** The stage, or the one task in its place: welcome, create your account, sign in. */
export type ScreenName = "welcome" | "redeem" | "signIn" | "stage";

/** What the view holds over the model's snapshot. */
export interface ViewState {
  /** The player asked to sign in to the active server: from the Account popover, or with
   * "I have an account" on Welcome or Join. */
  signIn: boolean;
}

export const NO_VIEW: ViewState = { signIn: false };

/** The server the stage shows, if there is one. */
export function activeServer(s: Snapshot): ServerView | undefined {
  return s.servers.find((v) => v.id === s.active);
}

/**
 * No server: welcome. The active server's invite waiting: create your account there. A sign-in
 * Play needs, or one the player asked for: sign in. Otherwise the stage. The invite waits on its own
 * server only, so the rail's other seals still lead to their stages.
 */
export function route(s: Snapshot, view: ViewState = NO_VIEW): ScreenName {
  if (s.servers.length === 0) {
    return "welcome";
  }
  if (s.pendingInvite !== null && s.pendingInvite.serverId === s.active) {
    return view.signIn ? "signIn" : "redeem";
  }
  if (s.needSignIn || (view.signIn && activeServer(s)?.signedIn === false)) {
    return "signIn";
  }
  return "stage";
}

export type PlayVariant =
  | "unsupported"
  | "checking"
  | "starting"
  | "running"
  | "maintenance"
  | "offline"
  // The server answered, but this launcher can't play there: its key, or its version.
  | "blocked"
  | "notReady"
  | "ready";

/** The stage's state, the first that holds, in this order. */
export function playVariant(s: Snapshot): PlayVariant {
  if (!s.platform.canPlay) {
    return "unsupported";
  }
  if (s.job?.kind === "checking") {
    return "checking";
  }
  if (s.game.kind === "starting" || s.game.kind === "running") {
    return s.game.kind;
  }
  const status = activeServer(s)?.status.kind;
  if (status === "maintenance") {
    return "maintenance";
  }
  if (status === "keyChanged" || status === "ownKey" || status === "incompatible") {
    return "blocked";
  }
  if (status !== "online") {
    return "offline";
  }
  return s.client.check.kind === "verified" ? "ready" : "notReady";
}

/**
 * PLAY kindles (R3): lit when ready; kindling while files verify or the game starts, filled to the
 * progress when there is one; closed when Play can't happen; an outline while the game runs.
 */
export type PlayLook =
  | { kind: "lit" }
  | { kind: "kindle"; task: "verifying" | "starting"; percent: number | null }
  | { kind: "closed" }
  | { kind: "running" };

export function playLook(s: Snapshot): PlayLook {
  switch (playVariant(s)) {
    case "ready":
      return { kind: "lit" };
    case "running":
      return { kind: "running" };
    case "checking":
      return { kind: "kindle", task: "verifying", percent: s.job?.kind === "checking" ? s.job.percent : null };
    case "starting":
      // The first start fetches umu-launcher: its download is the only progress a start has.
      return { kind: "kindle", task: "starting", percent: s.job?.kind === "fetching" ? s.job.percent : null };
    default:
      return { kind: "closed" };
  }
}

export type Ground = "warm" | "cool" | "plain";

/**
 * The stage's ground (R3): warm while PLAY is lit or the game runs; cool while it kindles, while
 * the game's files need something, and behind a task; plain ink when the server can't be played on.
 */
export function groundFor(s: Snapshot, screen: ScreenName): Ground {
  if (screen !== "stage") {
    return "cool";
  }
  const look = playLook(s);
  if (look.kind === "lit" || look.kind === "running") {
    return "warm";
  }
  return look.kind === "kindle" || playVariant(s) === "notReady" ? "cool" : "plain";
}

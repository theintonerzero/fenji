// Which screen a snapshot shows (the redesign spec §3), and the stage's states: PLAY's look (R3)
// and the ground behind it. The words for them are in words.ts.
import type { ServerView, Snapshot } from "./types";

/** The stage, or the one task in its place: welcome, create your account, sign in. */
export type ScreenName = "welcome" | "redeem" | "signIn" | "stage";

/** What the view holds over the model's snapshot. */
export interface ViewState {
  /**
   * The server the player asked to sign in to, from the Account popover or with "I have an
   * account" on Join or Welcome. `"added"` is the one Welcome is adding, until a snapshot names it.
   */
  signIn: number | "added" | null;
  /** The server PLAY asked a sign-in for (the model's `needSignIn`), so it shows there and only there. */
  playFor: number | null;
}

export const NO_VIEW: ViewState = { signIn: null, playFor: null };

/** The server the stage shows, if there is one. */
export function activeServer(s: Snapshot): ServerView | undefined {
  return s.servers.find((v) => v.id === s.active);
}

/**
 * No server: welcome. The active server's invite waiting: create your account there. A sign-in
 * Play needs, or one the player asked for: sign in. Otherwise the stage. Each waits on its own
 * server only, so the rail's other seals still lead to their stages.
 */
export function route(s: Snapshot, view: ViewState = NO_VIEW): ScreenName {
  if (s.servers.length === 0) {
    return "welcome";
  }
  const asked = s.active !== null && view.signIn === s.active && activeServer(s)?.signedIn === false;
  if (s.pendingInvite !== null && s.pendingInvite.serverId === s.active) {
    return asked ? "signIn" : "redeem";
  }
  const play = s.needSignIn && (view.playFor === null || view.playFor === s.active);
  return play || asked ? "signIn" : "stage";
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
  // Cancel start, whatever else runs meanwhile (#367).
  if (s.game.kind === "starting") {
    return "starting";
  }
  if (s.job?.kind === "checking") {
    return "checking";
  }
  if (s.game.kind === "running") {
    return "running";
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

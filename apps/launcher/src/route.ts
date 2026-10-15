// Which screen a snapshot shows on a tab (launcher spec §3.2), and which state Play's screen is in.
import type { ServerView, Snapshot } from "./types";

export type Tab = "play" | "servers" | "account" | "client" | "settings";

export type ScreenName =
  | "welcome"
  | "redeem"
  | "signIn"
  | "account"
  | "play"
  | "servers"
  | "clientSetup"
  | "clientWorking"
  | "clientRepair"
  | "clientReady"
  | "settings";

/** The server Play uses, if there is one. */
export function activeServer(s: Snapshot): ServerView | undefined {
  return s.servers.find((v) => v.id === s.active);
}

export function route(tab: Tab, s: Snapshot): ScreenName {
  if (tab === "settings") {
    return "settings";
  }
  if (s.servers.length === 0) {
    return "welcome";
  }
  if (s.pendingInvite !== null) {
    return "redeem";
  }
  if (s.needSignIn) {
    return "signIn";
  }
  switch (tab) {
    case "play":
      return "play";
    case "servers":
      return "servers";
    case "account":
      return activeServer(s)?.signedIn ? "account" : "signIn";
    case "client": {
      const job = s.job?.kind;
      if (job === "fetching" || job === "downloading") {
        return "clientWorking";
      }
      if (s.client.folder === null) {
        return "clientSetup";
      }
      return s.client.check.kind === "needsRepair" ? "clientRepair" : "clientReady";
    }
  }
}

export type PlayVariant =
  | "unsupported"
  | "checking"
  | "starting"
  | "running"
  | "maintenance"
  | "offline"
  | "notReady"
  | "ready";

/** Play's state, the first that holds, in this order. */
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
  if (status !== "online") {
    return "offline";
  }
  return s.client.check.kind === "verified" ? "ready" : "notReady";
}

/** What the blade shows (DESIGN.md, The blade): the player's copy, and how far a check has come. */
export type BladeFor =
  | { kind: "unlit" }
  | { kind: "climbing"; percent: number; label: string }
  | { kind: "lit" }
  | { kind: "bloom" };

export function bladeFor(s: Snapshot): BladeFor {
  const job = s.job;
  if (job?.kind === "checking") {
    return { kind: "climbing", percent: job.percent, label: `${job.filesDone} of ${job.filesTotal} files` };
  }
  if (job?.kind === "downloading" && job.percent !== null) {
    return { kind: "climbing", percent: job.percent, label: job.file ?? "Downloading" };
  }
  if (s.client.check.kind !== "verified" || !s.platform.canPlay) {
    return { kind: "unlit" };
  }
  const variant = playVariant(s);
  return variant === "ready" || variant === "starting" || variant === "running"
    ? { kind: "bloom" }
    : { kind: "lit" };
}

/** The ground behind the blade: warm while the active server is up and open, cool otherwise. */
export function groundFor(s: Snapshot): "warm" | "cool" {
  return activeServer(s)?.status.kind === "online" ? "warm" : "cool";
}

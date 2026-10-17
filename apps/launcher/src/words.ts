// The view's own words (the redesign spec R4, R5): one line of state, the Game chip, PLAY's label
// and who's playing. Plain words joined by " · "; times are the player's local time.
import type { PlayLook } from "./route";
import type { ServerView, Snapshot } from "./types";

export const JOIN = " · ";
const WEEKDAYS = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

function two(n: number): string {
  return n.toString().padStart(2, "0");
}

/** HH:MM, local time. */
export function clock(ms: number): string {
  const d = new Date(ms);
  return `${two(d.getHours())}:${two(d.getMinutes())}`;
}

function day(ms: number): number {
  const d = new Date(ms);
  return new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
}

const DAY = 24 * 60 * 60 * 1000;

/** Whole days from `ms` to `now`, by the calendar. */
function daysAgo(ms: number, now: number): number {
  return Math.round((day(now) - day(ms)) / DAY);
}

/** "21:00", or "Friday 21:00" when it isn't today. */
export function backAround(ms: number, now: number): string {
  return day(ms) === day(now) ? clock(ms) : `${WEEKDAYS[new Date(ms).getDay()]} ${clock(ms)}`;
}

/** "today 18:42", "yesterday 23:10" or "12 Oct", after "last seen". */
export function seen(ms: number, now: number): string {
  const days = daysAgo(ms, now);
  if (days === 0) {
    return `today ${clock(ms)}`;
  }
  if (days === 1) {
    return `yesterday ${clock(ms)}`;
  }
  const d = new Date(ms);
  return `${d.getDate()} ${MONTHS[d.getMonth()]}`;
}

/** "Today at 09:10", "Yesterday at 23:10" or "12 Oct": when a copy was verified. */
export function when(ms: number, now: number): string {
  const days = daysAgo(ms, now);
  if (days === 0) {
    return `Today at ${clock(ms)}`;
  }
  if (days === 1) {
    return `Yesterday at ${clock(ms)}`;
  }
  const d = new Date(ms);
  return `${d.getDate()} ${MONTHS[d.getMonth()]}`;
}

/** "78%". */
export function percent(p: number): string {
  return `${Math.round(p)}%`;
}

/**
 * The line under the server's name (R4): "Online · 3/20 players · 24 ms", "Maintenance · back
 * around 21:00", "Offline · last seen yesterday 23:10", "Key changed". The ping shows while PLAY is
 * lit, as the Ready board has it.
 */
export function stateLine(v: ServerView, now: number, lit: boolean): string {
  switch (v.status.kind) {
    case "online": {
      const players = v.players === null ? [] : [`${v.players.online}/${v.players.max} players`];
      const ping = lit && v.pingMs !== null ? [`${v.pingMs} ms`] : [];
      return ["Online", ...players, ...ping].join(JOIN);
    }
    case "maintenance": {
      const until = v.status.until;
      return ["Maintenance", ...(until === null ? [] : [`back around ${backAround(until, now)}`])].join(JOIN);
    }
    case "offline":
      return ["Offline", ...(v.lastSeenMs === null ? [] : [`last seen ${seen(v.lastSeenMs, now)}`])].join(JOIN);
    case "keyChanged":
      return "Key changed";
    case "ownKey":
      return "Not trusted yet";
    case "incompatible":
      return v.status.newer ? "Runs a newer Fenji" : "Expects another game build";
    case "unknown":
      return "Connecting…";
  }
}

/**
 * " · Germany": where the server is, when its host set it (hosting spec Amendment C, HC5), after
 * its facts in the states that show them: online, maintenance, and offline, with its last answer's.
 */
export function regionPart(v: ServerView): string {
  const shown = v.status.kind === "online" || v.status.kind === "maintenance" || v.status.kind === "offline";
  return shown && v.region !== null ? `${JOIN}${v.region}` : "";
}

/** " · Fenji 0.5.3": the Fenji the server runs, from its last answer, after its address in the top
 * bar; nothing when it never said (the boards, R1). */
export function versionPart(v: ServerView | null | undefined): string {
  return v?.version ? `${JOIN}Fenji ${v.version}` : "";
}

/** A seal's tip in the rail, after the server's name: its players, or its state, and its region. */
export function sealLine(v: ServerView): string {
  switch (v.status.kind) {
    case "online":
      return (v.players === null ? "Online" : `${v.players.online}/${v.players.max} players`) + regionPart(v);
    case "maintenance":
      return "Maintenance" + regionPart(v);
    case "offline":
      return "Offline" + regionPart(v);
    case "unknown":
      return "Connecting…";
    default:
      return stateLine(v, 0, false);
  }
}

export type Tone = "ok" | "warn" | "quiet";

/** The Game chip in the top bar: the player's copy in two or three words, with its dot's tone. */
export function gameChip(s: Snapshot): { label: string; tone: Tone } {
  const job = s.job;
  if (job?.kind === "checking") {
    return { label: "Verifying", tone: "quiet" };
  }
  if (job?.kind === "downloading") {
    // A sign-in waits on the player, and the popover may be closed (the final review, I6).
    if (job.waitingForSteam) {
      return { label: "Sign in to Steam", tone: "warn" };
    }
    const words = job.repair ? "Repairing" : "Downloading";
    return { label: job.percent === null ? words : `${words} ${percent(job.percent)}`, tone: "quiet" };
  }
  // DepotDownloader comes first on a first download; umu-launcher on a first start is PLAY's.
  if (job?.kind === "fetching" && s.game.kind !== "starting") {
    return { label: "Downloading", tone: "quiet" };
  }
  const { client } = s;
  if (client.folder === null) {
    return { label: "Set up the game", tone: "quiet" };
  }
  switch (client.check.kind) {
    case "verified":
      return { label: "Game ready", tone: "ok" };
    case "needsRepair": {
      const n = client.check.files.length;
      return { label: n === 1 ? "1 file needs repair" : `${n} files need repair`, tone: "warn" };
    }
    case "folderMissing":
      return { label: "Game folder missing", tone: "warn" };
    case "none":
      return { label: "Not verified", tone: "quiet" };
  }
}

/** PLAY's label: "Play", "Verifying 78%", "Starting" or "Running". */
export function playWords(look: PlayLook): string {
  switch (look.kind) {
    case "lit":
    case "closed":
      return "Play";
    case "running":
      return "Running";
    case "kindle":
      if (look.task === "starting") {
        return "Starting";
      }
      return look.percent === null ? "Verifying" : `Verifying ${percent(look.percent)}`;
  }
}

/** The system's own name, for the line that says Play isn't supported on it yet. */
export function systemName(os: string): string {
  switch (os) {
    case "windows":
      return "Windows";
    case "macos":
      return "macOS";
    case "linux":
      return "Linux";
    default:
      return "this system";
  }
}

/** The launcher's version, then an update: "0.5.1 · up to date", "0.5.1 · 0.5.2 ready". */
export function versionLine(s: Snapshot): string {
  const { update } = s;
  if (update.checking) {
    return `${update.current}${JOIN}checking…`;
  }
  if (update.available === null) {
    return update.upToDate ? `${update.current}${JOIN}up to date` : update.current;
  }
  return `${update.current}${JOIN}${update.available} ${update.ready ? "ready" : "available"}`;
}

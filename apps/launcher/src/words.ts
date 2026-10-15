// The view's own sentences (launcher spec §3.3, §3.12; DESIGN.md, Status): plain words for what
// is true, joined by " · ". Times are the player's local time.
import { activeServer } from "./route";
import type { ServerView, Snapshot } from "./types";

const JOIN = " · ";
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

/** "21:00", or "Friday 21:00" when it isn't today. */
export function backAround(ms: number, now: number): string {
  return day(ms) === day(now) ? clock(ms) : `${WEEKDAYS[new Date(ms).getDay()]} ${clock(ms)}`;
}

/** "today at 18:42", "yesterday at 23:10", or "on 12 Oct". */
export function seen(ms: number, now: number): string {
  const days = Math.round((day(now) - day(ms)) / DAY);
  if (days === 0) {
    return `today at ${clock(ms)}`;
  }
  if (days === 1) {
    return `yesterday at ${clock(ms)}`;
  }
  const d = new Date(ms);
  return `on ${d.getDate()} ${MONTHS[d.getMonth()]}`;
}

function friends(n: number): string {
  return n === 1 ? "1 friend" : `${n} friends`;
}

/** Who's playing: friends by name while signed in on the active server, else the server's count. */
function playing(v: ServerView, s: Snapshot, row: boolean): string[] {
  const own = !row && v.id === s.active && v.signedIn && s.lobby !== null;
  if (own) {
    const others = s.lobby!.filter((name) => name !== v.username).length;
    if (s.game.kind === "running") {
      return [others === 0 ? "you're playing" : `you and ${friends(others)} playing`];
    }
    return [others === 0 ? "no one playing yet" : `${friends(others)} playing`];
  }
  return v.players ? [`${v.players.online} of ${v.players.max} playing`] : [];
}

function pieces(v: ServerView, s: Snapshot, now: number, row: boolean): string[] {
  const host = v.host ? [`hosted by ${v.host}`] : [];
  switch (v.status.kind) {
    case "online": {
      const ping = row && v.pingMs !== null ? [`${v.pingMs} ms`] : [];
      return ["Online", ...playing(v, s, row), ...ping, ...host];
    }
    case "maintenance": {
      const until = v.status.until;
      const back = until === null ? [] : [`back around ${backAround(until, now)}`];
      return ["In maintenance", ...back, ...host];
    }
    case "offline":
      return [`Can't reach ${v.name}`, ...(v.lastSeenMs === null ? [] : [`last seen ${seen(v.lastSeenMs, now)}`])];
    case "keyChanged":
      return [`${v.name}'s key has changed`];
    case "ownKey":
      return [`${v.name} uses its own key`];
    case "incompatible":
      return [v.status.newer ? `${v.name} runs a newer Fenji` : `${v.name} expects another game build`];
    case "unknown":
      return [`Checking ${v.name}…`];
  }
}

/** The line under the active server's name. */
export function statusLine(s: Snapshot, now: number): string {
  const v = activeServer(s);
  return v ? pieces(v, s, now, false).join(JOIN) : "No server yet";
}

/** A row of the Servers list, with the ping. */
export function serverLine(v: ServerView, s: Snapshot, now: number): string {
  return pieces(v, s, now, true).join(JOIN);
}

/** "78%". */
export function percent(p: number): string {
  return `${Math.round(p)}%`;
}

/** "164 of 210 files". */
export function filesOf(done: number, total: number): string {
  return `${done} of ${total} ${total === 1 ? "file" : "files"}`;
}

/**
 * The client's meta line: "Client verified · build 10015727 · 210 files", or what's wrong. Under
 * the column's "Your client" heading it starts "Verified".
 */
export function clientLine(s: Snapshot, underHeading = false): string {
  const { client } = s;
  const build = `build ${client.build}`;
  const files = `${client.files} files`;
  switch (client.check.kind) {
    case "verified":
      return [underHeading ? "Verified" : "Client verified", build, files].join(JOIN);
    case "needsRepair": {
      const wrong = client.check.files.length;
      return [`${client.check.fine} fine`, `${wrong} to repair`, build].join(JOIN);
    }
    case "folderMissing":
      return ["The game folder is missing", build].join(JOIN);
    case "none":
      return [client.folder === null ? "No game folder yet" : "Not checked yet", build, files].join(JOIN);
  }
}

/** The column's foot: the launcher's version, and a downloaded or available update. */
export function versionLine(s: Snapshot): string {
  const { update } = s;
  if (update.available === null) {
    return update.upToDate ? `Launcher ${update.current} · up to date` : `Launcher ${update.current}`;
  }
  return `Launcher ${update.current} · ${update.available} ${update.ready ? "ready" : "available"}`;
}

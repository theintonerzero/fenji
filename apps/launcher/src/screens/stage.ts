// The stage (R1, R3, R4; the boards' Playing and Can't play screens): the chosen server's name,
// one line of state, the host's notice, PLAY with at most one action beside it, a line only when
// one is needed, and who's playing. It mounts once and updates in place, so PLAY and the action
// beside it keep a player's focus while a check's progress streams in.
import type { Ctx, Screen } from "../app";
import { h, type Child } from "../dom";
import { activeServer, playLook, playVariant, type PlayVariant } from "../route";
import type { Intent, ServerView, Snapshot } from "../types";
import { button, setPlay } from "../ui/button";
import { icons } from "../ui/icons";
import { friends } from "../ui/people";
import { clock, stateLine, systemName } from "../words";
import { noticeSlot, renew } from "./parts";

type Tone = "ok" | "warn" | "quiet" | "error";

function toneFor(v: ServerView): Tone {
  switch (v.status.kind) {
    case "online":
      return "ok";
    case "maintenance":
      return "warn";
    case "unknown":
      return "quiet";
    default:
      return "error";
  }
}

/** A dot by state, or for an error the circled exclamation: a colour never goes without words. */
function mark(tone: Tone): Element {
  if (tone === "error") {
    const bang = icons.error(18);
    bang.classList.add("bang");
    return bang;
  }
  return h("span", { class: `dot dot-${tone}`, "aria-hidden": "true" });
}

function gold(name: string): HTMLElement {
  return h("span", { class: "gold" }, name);
}

/** The one action beside PLAY (R4): its words, whether it's quiet words or an outline, and what it does. */
interface Beside {
  words: string;
  quiet: boolean;
  act(): void;
}

function besidePlay(variant: PlayVariant, s: Snapshot, v: ServerView, ctx: Ctx): Beside | null {
  const send = (intent: Intent) => () => void ctx.send(intent);
  switch (variant) {
    case "checking":
      return { words: "Stop", quiet: true, act: send({ type: "stopCheck" }) };
    case "starting":
      return { words: "Cancel", quiet: false, act: send({ type: "cancelStart" }) };
    case "maintenance":
      return { words: "Refresh", quiet: false, act: send({ type: "refresh" }) };
    case "offline":
      return { words: "Retry", quiet: false, act: send({ type: "refresh" }) };
    case "blocked":
      switch (v.status.kind) {
        case "keyChanged":
          return { words: "Paste new link", quiet: false, act: () => ctx.addServer() };
        case "ownKey":
          return { words: "Paste link", quiet: false, act: () => ctx.addServer() };
        default:
          return { words: "Check for updates", quiet: false, act: send({ type: "checkUpdate" }) };
      }
    case "notReady": {
      // A download or a check shows on the Game chip, which opens its progress.
      if (s.job !== null) {
        return null;
      }
      const { client } = s;
      const words =
        client.folder === null
          ? "Set up the game"
          : client.check.kind === "needsRepair"
            ? "Repair"
            : client.check.kind === "folderMissing"
              ? "Find the game"
              : "Verify";
      return { words, quiet: false, act: () => ctx.open("game") };
    }
    default:
      return null;
  }
}

/** The host's line: their notice, led by their name in gold, or what a server this launcher can't
 * play on needs (R4). */
function hostLine(variant: PlayVariant, v: ServerView): Child[] | null {
  const host = v.host === null ? "your host" : gold(v.host);
  switch (variant) {
    case "blocked":
      switch (v.status.kind) {
        case "keyChanged":
          return ["This isn't the server you trusted. Ask ", host, " for a new link."];
        case "ownKey":
          return ["Paste the invite link ", host, " sent you."];
        case "incompatible":
          return v.status.newer ? ["Update the launcher to play here."] : ["Ask ", host, " which game build to use."];
        default:
          return null;
      }
    case "offline":
    case "unsupported":
      return null;
    default: {
      const text = v.status.kind === "maintenance" && v.status.message !== "" ? v.status.message : v.motd;
      if (text === "") {
        return null;
      }
      return [v.host === null ? null : [gold(`${v.host}:`), " "], text];
    }
  }
}

/** The line under PLAY, only when one is needed. */
function metaWords(variant: PlayVariant, s: Snapshot): string | null {
  switch (variant) {
    case "starting":
      if (s.job?.kind === "fetching") {
        return `Getting ${s.job.tool} (first start only)`;
      }
      return s.game.kind === "starting" && s.game.firstStart ? "Setting up Proton (first start only)" : null;
    case "running":
      return s.game.kind === "running" ? `Since ${clock(s.game.sinceMs)}` : null;
    case "unsupported":
      return `Playing on ${systemName(s.platform.os)} isn't supported yet.`;
    default:
      return null;
  }
}

/** What to try when the server doesn't answer: the address, the host's server, the internet. */
function tryThese(v: ServerView): HTMLElement[] {
  const host = (): Child => (v.host === null ? "your host" : gold(v.host));
  return [
    h("li", {}, "Check the address, ", h("span", { class: "mono" }, v.address), ", with ", host(), "."),
    h("li", {}, "Make sure ", host(), "'s server is running."),
    h("li", {}, "Check your internet connection."),
  ];
}

/** The lobby shows where a player could be in it. */
const LOBBY: ReadonlySet<PlayVariant> = new Set(["ready", "notReady", "checking", "starting", "running", "unsupported"]);

let next = 0;

export function stage(root: HTMLElement, ctx: Ctx): Screen {
  next += 1;
  const stateId = `stage-${next}-state`;
  const tryId = `stage-${next}-try`;
  const name = h("h1", { class: "name" });
  const state = h("p", { class: "state", id: stateId, role: "status" });
  const host = h("p", { class: "host", hidden: true });
  const playButton = button.play(() => void ctx.send({ type: "play" }));
  playButton.setAttribute("aria-describedby", stateId);
  let act: (() => void) | null = null;
  const outline = button.secondary("", () => act?.(), true);
  const quiet = button.quiet("", () => act?.());
  const whatToTry = button.link("What to try", () => {
    const open = list.hidden;
    list.hidden = !open;
    whatToTry.setAttribute("aria-expanded", String(open));
  });
  whatToTry.setAttribute("aria-controls", tryId);
  whatToTry.setAttribute("aria-expanded", "false");
  const progress = h("div", { class: "visually-hidden", role: "progressbar", "aria-valuemin": "0", "aria-valuemax": "100" });
  const list = h("ul", { class: "try", id: tryId, hidden: true });
  const slot = noticeSlot(ctx);
  const meta = h("p", { class: "meta", hidden: true });
  const crowd = h("div", {});
  root.append(
    h(
      "div",
      { class: "group" },
      name,
      state,
      host,
      h("div", { class: "row" }, playButton, outline, quiet, whatToTry),
      progress,
      list,
      slot.el,
      meta,
      crowd,
    ),
  );
  let offlineKey = "";

  return {
    update(s) {
      const v = activeServer(s);
      if (v === undefined) {
        return;
      }
      const variant = playVariant(s);
      const look = playLook(s);
      if (name.textContent !== v.name) {
        name.textContent = v.name;
      }
      const line = stateLine(v, Date.now(), look.kind === "lit");
      renew(state, `${toneFor(v)}\n${line}`, () => [mark(toneFor(v)), line]);

      const words = hostLine(variant, v);
      host.hidden = words === null;
      renew(host, JSON.stringify([variant, v.status, v.host, v.motd]), () => words ?? []);

      setPlay(playButton, look);
      if (look.kind === "kindle") {
        progress.setAttribute("aria-label", look.task === "verifying" ? "Verifying" : "Starting");
        if (look.percent === null) {
          progress.removeAttribute("aria-valuenow");
        } else {
          progress.setAttribute("aria-valuenow", String(Math.round(look.percent)));
        }
        progress.hidden = false;
      } else {
        progress.hidden = true;
      }

      const next = besidePlay(variant, s, v, ctx);
      act = next?.act ?? null;
      outline.hidden = next === null || next.quiet;
      quiet.hidden = next === null || !next.quiet;
      const shownButton = next === null ? null : next.quiet ? quiet : outline;
      if (shownButton !== null && shownButton.textContent !== next!.words) {
        shownButton.textContent = next!.words;
      }

      const offline = variant === "offline";
      whatToTry.hidden = !offline;
      const key = offline ? JSON.stringify([v.address, v.host]) : "";
      if (key !== offlineKey) {
        offlineKey = key;
        list.hidden = true;
        whatToTry.setAttribute("aria-expanded", "false");
        list.replaceChildren(...(offline ? tryThese(v) : []));
      }

      const logs =
        s.notice?.error && s.notice.text.endsWith("See the logs.")
          ? [button.link("Open logs", () => void ctx.send({ type: "openLogFolder" }))]
          : [];
      slot.update(s, logs);

      const metaLine = metaWords(variant, s);
      meta.hidden = metaLine === null;
      if (metaLine !== null && meta.textContent !== metaLine) {
        meta.textContent = metaLine;
      }

      const lobby = LOBBY.has(variant) && v.signedIn && s.lobby !== null ? s.lobby.filter((n) => n !== v.username) : [];
      const you = variant === "running" && v.signedIn ? v.username : null;
      renew(crowd, JSON.stringify([lobby, you]), () => {
        const row = friends(lobby, you);
        return row === null ? [] : [row];
      });
    },
    destroy() {
      root.replaceChildren();
    },
  };
}

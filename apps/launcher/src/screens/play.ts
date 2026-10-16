// Play (launcher spec §3.2, §3.7, §3.12; the boards EmberSword, Checking, Playing, Maintenance,
// Offline and Unsupported): one hero composition, its state from playVariant. The screen mounts
// once and updates in place, so PLAY and the button beside it stay the same elements across
// snapshots (plan LP4) and keep a player's focus while a check's progress streams in.
import type { Ctx, Screen } from "../app";
import { h, type Child } from "../dom";
import { activeServer, bladeFor, groundFor, playVariant, type PlayVariant, type Tab } from "../route";
import type { Intent, ServerView, Snapshot } from "../types";
import { blade } from "../ui/blade";
import { button, setPlay, type PlayState } from "../ui/button";
import type { Dialog } from "../ui/dialog";
import { section } from "../ui/column";
import { icons } from "../ui/icons";
import { people } from "../ui/people";
import { statusMark } from "../ui/status";
import { clientLine, clock, statusLine, versionLine } from "../words";
import { contextColumn, noticeSlot, renew, serverCard, type Fact } from "./parts";
import { addDialog } from "./servers";

function playState(variant: PlayVariant): PlayState {
  switch (variant) {
    case "ready":
      return { kind: "ready" };
    case "starting":
      return { kind: "starting" };
    case "running":
      return { kind: "running" };
    default:
      return { kind: "disabled" };
  }
}

const QUICK_CHECK: Intent = { type: "check", level: "quick" };

/** The button beside PLAY, if there is one: its words and what it sends. */
function besidePlay(variant: PlayVariant, s: Snapshot): [string, Intent] | null {
  switch (variant) {
    case "ready":
      return ["Check files", QUICK_CHECK];
    case "checking":
      return ["Stop checking", { type: "stopCheck" }];
    case "starting":
      return ["Cancel start", { type: "cancelStart" }];
    case "maintenance":
      return ["Check again", { type: "refresh" }];
    case "unsupported":
    case "notReady":
      if (s.job?.kind === "checking") {
        return ["Stop checking", { type: "stopCheck" }];
      }
      return s.client.folder === null || s.job !== null ? null : ["Check files", QUICK_CHECK];
    default:
      return null;
  }
}

/** A tab's name as a text button inside a sentence. */
function tabLink(ctx: Ctx, words: string, tab: Tab): HTMLButtonElement {
  return h("button", { class: "inline-link", type: "button", onclick: () => ctx.setTab(tab) }, words);
}

function ok(size: number): Element {
  const icon = icons.check(size);
  icon.classList.add("icon-ok");
  return icon;
}

/** A bullet: its dot, then the words in one box, so a link inside flows with them. */
function bullet(...words: Child[]): HTMLLIElement {
  return h("li", {}, h("span", {}, ...words));
}

/** What to try when the server doesn't answer (the Offline board), or what a server this launcher
 * can't play on needs (§3.12): the words under the status line, which don't repeat it. */
function trouble(v: ServerView, ctx: Ctx): Child[] {
  const host = v.host ?? "your host";
  switch (v.status.kind) {
    case "offline":
      return [
        h("h2", { class: "try-heading" }, "What to try"),
        h(
          "ul",
          { class: "bullets" },
          bullet(`Check the address: ${v.address}. `, tabLink(ctx, "Change it", "servers"), ` if ${host} gave you a different one.`),
          bullet(
            `If ${v.name} runs on your own network, make sure the host's server is running. Your internet isn't needed for that.`,
          ),
          bullet(`Otherwise, check your internet connection, or ask ${host} whether ${v.name} is up.`),
        ),
      ];
    case "keyChanged":
      return [h("p", { class: "prose trouble-words" }, `Ask ${host} for the new invite link, then add it here.`)];
    case "ownKey":
      return [h("p", { class: "prose trouble-words" }, `Add the invite link ${host} sent you: it carries the server's key.`)];
    case "incompatible":
      return [
        h(
          "p",
          { class: "prose trouble-words" },
          v.status.newer
            ? `Update the launcher to play on ${v.name}.`
            : `Check for a launcher update, or ask ${host} which build to use.`,
        ),
      ];
    default:
      return [];
  }
}

/** The one thing to do on a server this launcher can't play on (the finish review's fix 1): a new
 * link for its key, or a launcher update for its version. "Try again" would never cure either. */
function cure(v: ServerView): string | null {
  switch (v.status.kind) {
    case "keyChanged":
      return "Add the new link";
    case "ownKey":
      return "Add the link";
    case "incompatible":
      return "Check for updates";
    default:
      return null;
  }
}

/** Where a player without a verified copy goes next: always the Client tab. */
function clientStep(s: Snapshot): string {
  if (s.job?.kind === "downloading" || s.job?.kind === "fetching") {
    return "Follow the download on the Client tab";
  }
  if (s.client.folder === null) {
    return "Set up the game on the Client tab";
  }
  switch (s.client.check.kind) {
    case "needsRepair":
      return "Repair the game on the Client tab";
    case "folderMissing":
      return "Find the game folder again on the Client tab";
    default:
      return "Check the game's files on the Client tab";
  }
}

/** The line under the buttons, which says why PLAY is closed when it is. */
function meta(variant: PlayVariant, s: Snapshot, v: ServerView, ctx: Ctx): Child[] {
  const host = v.host ?? "your host";
  switch (variant) {
    case "ready":
      return [h("p", { class: "meta" }, ok(13), clientLine(s))];
    case "checking":
      return [h("p", { class: "meta" }, icons.clock(14), "Play opens when every file checks out")];
    case "starting":
      return s.game.kind === "starting" && s.game.firstStart
        ? [h("p", { class: "meta" }, icons.clock(14), "The first start sets up Proton, which takes a few minutes")]
        : [];
    case "running":
      return [
        h(
          "div",
          { class: "running" },
          ok(13),
          h(
            "div",
            {},
            h("p", { class: "running-title" }, "Swords of Legends Online is running"),
            v.signedIn && v.username !== null
              ? h("p", { class: "running-line" }, `Signed in to ${v.name} as ${v.username}`)
              : null,
          ),
        ),
      ];
    case "maintenance": {
      const checked = v.lastSeenMs === null ? [] : [`Last checked at ${clock(v.lastSeenMs)}`];
      return [
        h("p", { class: "meta" }, icons.clock(14), [...checked, `Play opens when ${host} ends maintenance`].join(" · ")),
      ];
    }
    case "offline":
      return v.status.kind === "offline"
        ? [h("p", { class: "meta" }, icons.clock(14), `No answer from ${v.address}`)]
        : [];
    case "blocked":
    case "unsupported":
      return [];
    case "notReady":
      return [h("p", { class: "meta" }, icons.info(14), button.link(clientStep(s), () => ctx.setTab("client")))];
  }
}

const WORKS_HERE: [item: string, tab: string, to: Tab][] = [
  ["Create your account", "Account tab", "account"],
  ["Check your game folder", "Client tab", "client"],
  ["See the server's state", "Servers tab", "servers"],
];

/** Off Linux (the Unsupported board): exactly DESIGN.md's words, and what still works here. */
function unsupportedCard(ctx: Ctx): HTMLElement {
  const info = icons.info(18);
  info.classList.add("icon-info");
  return h(
    "section",
    { class: "card unsupported", "aria-labelledby": "play-unsupported" },
    h("h2", { class: "unsupported-title" }, info, h("span", { id: "play-unsupported" }, "Fenji can't start the client on this system yet")),
    h("p", { class: "unsupported-lede" }, "Everything else in the launcher works here:"),
    h(
      "ul",
      { class: "action-rows" },
      WORKS_HERE.map(([item, tab, to]) =>
        h(
          "li",
          { class: "action-row" },
          ok(13),
          h("span", { class: "action-row-item" }, item),
          button.link([tab, icons.chevron(14)], () => ctx.setTab(to)),
        ),
      ),
    ),
  );
}

/** The column (DESIGN.md, The context column): who's playing, the server's facts, and the client
 * while the server can't be played on. */
function columnSections(variant: PlayVariant, s: Snapshot, v: ServerView, ctx: Ctx): Child[] {
  const now = Date.now();
  const closed = variant === "maintenance" || variant === "offline" || variant === "blocked";
  // "Playing now" heads the list itself (plan LP5); over a sentence saying why there's no list, a
  // heading that claims one would contradict it (the finish review's fix 8).
  const host = v.host ?? "your host";
  const playing =
    variant === "maintenance"
      ? section("Who's playing", h("p", { class: "people-empty" }, `The lobby is closed while ${v.name} is in maintenance.`))
      : variant === "offline"
        ? section("Who's playing", h("p", { class: "people-empty" }, `Who's online shows again once ${v.name} answers.`))
        : variant === "blocked"
          ? section(
              "Who's playing",
              h(
                "p",
                { class: "people-empty" },
                v.status.kind === "incompatible"
                  ? `Who's online shows once this launcher can play on ${v.name}.`
                  : `Who's online shows once you've added ${host}'s ${v.status.kind === "keyChanged" ? "new " : ""}link.`,
              ),
            )
          : people(v.signedIn ? s.lobby : null, v.username, v.host);
  const rows: Fact[] =
    variant === "maintenance"
      ? ["status", "back", "address", "version"]
      : variant === "offline"
        ? ["status", "lastSeen", "address", "version"]
        : variant === "blocked"
          ? ["status", "address", "version"]
          : ["address", "version", "players"];
  const verified = s.client.check.kind === "verified";
  const client = closed
    ? section(
        "Your client",
        h("p", { class: "client-fact" }, verified ? ok(13) : null, clientLine(s, true)),
        variant === "maintenance"
          ? verified
            ? h("p", { class: "client-note" }, `Ready to play when ${v.name} is back.`)
            : null
          : h(
              "p",
              { class: "client-note" },
              "Checking your files works without the server, on the ",
              tabLink(ctx, "Client", "client"),
              " tab.",
            ),
      )
    : null;
  return [playing, serverCard(v, rows, now), client];
}

/** What the column shows, so it's rebuilt only when that changes (not on each ping). */
function columnKey(variant: PlayVariant, s: Snapshot, v: ServerView): string {
  return JSON.stringify([
    variant,
    v.name,
    v.host,
    v.address,
    v.status,
    v.version,
    v.players,
    v.lastSeenMs,
    v.signedIn,
    v.username,
    s.lobby,
    clientLine(s, true),
    s.client.check.kind,
  ]);
}

export function play(root: HTMLElement, ctx: Ctx): Screen {
  const art = blade({ kind: "unlit" });
  const title = h("h1", { class: "display" });
  const status = h("p", { class: "status-line", role: "status" });
  const host = h("p", { class: "notice prose" });
  const troubleEl = h("div", { class: "trouble" });
  const slot = noticeSlot(ctx);
  const playButton = button.play({ kind: "disabled" }, () => void ctx.send({ type: "play" }));
  const retry = button.primary("Try again", () => void ctx.send({ type: "refresh" }));
  // A server this launcher can't play on: Add server opens here, or the launcher checks for updates.
  let open: Dialog | null = null;
  let blockedBy: ServerView["status"]["kind"] | null = null;
  const fix = button.primary("", () => {
    if (blockedBy === "incompatible") {
      void ctx.send({ type: "checkUpdate" });
    } else if (blockedBy !== null) {
      open?.close();
      open = addDialog(ctx, slot, () => {
        open = null;
      });
      document.body.append(open.el);
    }
  });
  let besideIntent: Intent | null = null;
  const beside = button.secondary("", () => {
    if (besideIntent !== null) {
      void ctx.send(besideIntent);
    }
  });
  const why = h("div", { class: "play-meta", id: "play-why" });
  const unsupported = h("div", {});
  const side = contextColumn();
  root.append(
    h(
      "main",
      { class: "main" },
      art,
      h(
        "div",
        { class: "hero" },
        title,
        status,
        host,
        troubleEl,
        slot.el,
        h("div", { class: "actions" }, playButton, retry, fix, beside),
        why,
        unsupported,
      ),
    ),
    side.el,
  );

  return {
    update(s) {
      const v = activeServer(s);
      const variant = playVariant(s);
      art.update(bladeFor(s), groundFor(s));
      const name = v?.name ?? "Play";
      if (title.textContent !== name) {
        title.textContent = name;
      }
      const line = statusLine(s, Date.now());
      renew(status, `${v?.status.kind}\n${line}`, () => [statusMark(v), line]);

      const notice =
        v === undefined || variant === "offline" || variant === "blocked" || variant === "unsupported"
          ? ""
          : v.status.kind === "maintenance" && v.status.message !== ""
            ? v.status.message
            : v.motd;
      host.hidden = notice === "";
      renew(host, `${v?.host}\n${notice}`, () => [v?.host ? [h("strong", {}, `${v.host}:`), " "] : null, notice]);
      const troubled = (variant === "offline" || variant === "blocked") && v !== undefined;
      renew(troubleEl, troubled ? JSON.stringify([v.status, v.name, v.host, v.address]) : "", () =>
        troubled ? trouble(v, ctx) : [],
      );
      slot.update(
        s,
        s.game.kind === "ended" && s.game.code !== 0
          ? [button.link("Open the log folder", () => void ctx.send({ type: "openLogFolder" }))]
          : [],
      );

      setPlay(playButton, v === undefined ? { kind: "disabled" } : playState(variant));
      playButton.hidden = variant === "offline" || variant === "blocked";
      retry.hidden = variant !== "offline";
      retry.disabled = variant !== "offline";
      const fixWords = variant === "blocked" && v !== undefined ? cure(v) : null;
      blockedBy = fixWords === null || v === undefined ? null : v.status.kind;
      fix.hidden = fixWords === null;
      fix.disabled = fixWords === null;
      if (fixWords !== null && fix.textContent !== fixWords) {
        fix.textContent = fixWords;
      }
      const next = v === undefined ? null : besidePlay(variant, s);
      beside.hidden = next === null;
      besideIntent = next?.[1] ?? null;
      if (next !== null && beside.textContent !== next[0]) {
        beside.textContent = next[0];
      }

      renew(why, v === undefined ? "none" : JSON.stringify([variant, s.client, s.job?.kind, s.game, v.status, v.lastSeenMs, v.signedIn]), () =>
        v === undefined
          ? [h("p", { class: "meta" }, icons.info(14), button.link("Choose a server on the Servers tab", () => ctx.setTab("servers")))]
          : meta(variant, s, v, ctx),
      );
      renew(unsupported, variant === "unsupported" ? "unsupported" : "", () =>
        variant === "unsupported" ? [unsupportedCard(ctx)] : [],
      );
      const describedBy = variant === "unsupported" ? "play-unsupported" : playButton.disabled && why.childElementCount > 0 ? "play-why" : null;
      if (describedBy === null) {
        playButton.removeAttribute("aria-describedby");
      } else {
        playButton.setAttribute("aria-describedby", describedBy);
      }

      side.show(
        v === undefined ? "none" : columnKey(variant, s, v),
        () => (v === undefined ? [] : columnSections(variant, s, v, ctx)),
        versionLine(s),
      );
    },
    destroy() {
      open?.close();
      root.replaceChildren();
    },
  };
}

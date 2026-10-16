// What the Client tab's screens share (the boards ClientSetup, Downloading and Repair): the hero
// they stand in, beside the blade, and the column's "Game client" card.
import type { Ctx } from "../app";
import { h, type Child } from "../dom";
import { activeServer } from "../route";
import type { Snapshot, ToolLinkView } from "../types";
import { button } from "../ui/button";
import { facts } from "../ui/card";
import { cardSection } from "../ui/column";
import { dialog, type Dialog } from "../ui/dialog";
import { icons } from "../ui/icons";
import { people } from "../ui/people";
import { seen, versionLine } from "../words";
import { contextColumn } from "./parts";

export const GAME = "Swords of Legends Online";

/** A folder in Geist Mono, as DESIGN.md writes paths. */
/** A path as the player knows it, the home folder as `~` (the boards' "~/Games/SoLO"), so a
 * screenshot doesn't carry the player's name. For display only: intents carry the whole path. */
export function shown(text: string, home: string | null): string {
  if (home === null || home === "" || !(text === home || text.startsWith(`${home}/`))) {
    return text;
  }
  return `~${text.slice(home.length)}`;
}

export function path(text: string, home: string | null = null): HTMLElement {
  return h("span", { class: "mono path" }, shown(text, home));
}

/** A path's last folders ("…/Games/SoLO") where a narrow place would cut it, the whole in its title. */
export function pathEnd(whole: string, max: number, home: string | null = null): HTMLElement {
  const text = shown(whole, home);
  if (text.length <= max) {
    return path(text);
  }
  const parts = text.split(/(?=[/\\])/);
  let kept = parts[parts.length - 1] ?? text;
  for (let i = parts.length - 2; i >= 0 && parts[i]!.length + kept.length + 1 <= max; i -= 1) {
    kept = parts[i] + kept;
  }
  return h("span", { class: "mono path", title: text }, `…${kept}`);
}

/** When the last check ran, and how far it went: "every file, today at 20:10". */
export function lastCheck(s: Snapshot, now: number): string | null {
  const { check } = s.client;
  switch (check.kind) {
    case "verified":
      return `${check.level === "full" ? "Every file" : "Quick check"}, ${seen(check.atMs, now)}`;
    case "needsRepair":
      return `${check.fine} fine · ${check.files.length} to repair`;
    default:
      return null;
  }
}

/** The column on the Client tab: who's playing, then the copy's facts. */
export function clientColumn(): { el: HTMLElement; update(s: Snapshot): void } {
  const side = contextColumn();
  return {
    el: side.el,
    update(s) {
      const v = activeServer(s);
      const now = Date.now();
      const check = lastCheck(s, now);
      const source = s.job?.kind === "downloading" ? "Your Steam library" : null;
      side.show(
        JSON.stringify([v?.signedIn, v?.username, v?.host, s.lobby, s.client, source, check]),
        () => {
          const rows: [string, Child][] = [
            ["Game", GAME],
            ["Build", `${s.client.build} · ${s.client.files} files`],
            ["Folder", s.client.folder === null ? "Not chosen yet" : path(s.client.folder, s.platform.home)],
          ];
          if (source !== null) {
            rows.push(["Source", source]);
          }
          if (check !== null) {
            rows.push(["Last check", check]);
          }
          return [
            v === undefined ? null : people(v.signedIn ? s.lobby : null, v.username, v.host),
            cardSection("Game client", facts(rows)),
          ];
        },
        versionLine(s),
      );
    },
  };
}

/** The command, selected, for a player to copy by hand when the clipboard isn't there. */
function select(el: Element): void {
  const range = document.createRange();
  range.selectNodeContents(el);
  const selection = window.getSelection();
  selection?.removeAllRanges();
  selection?.addRange(range);
}

/**
 * DepotDownloader's command for the player's own terminal (spec §3.6): their folder and the pinned
 * build filled in, their own Steam login left for them to type. Without Fenji's own DepotDownloader,
 * `get` says where to get it, the link on a line of its own. Then they choose the folder it
 * downloaded to (setup), or check the files it repaired (Repair), a quick check that hashes those
 * in full (#348).
 */
export function commandDialog(
  ctx: Ctx,
  command: string,
  get: ToolLinkView | null,
  then: "chooseFolder" | "check",
  done: () => void,
): Dialog {
  const code = h("code", {}, command);
  const copy = button.secondary(
    "Copy",
    () => {
      const write = navigator.clipboard?.writeText(command);
      if (write === undefined) {
        select(code);
      } else {
        write.then(
          () => {
            copy.textContent = "Copied";
          },
          () => select(code),
        );
      }
    },
    "small",
  );
  const close = (): void => {
    shown.close();
    done();
  };
  const shown = dialog({
    title: "Run DepotDownloader yourself",
    icon: icons.download(20),
    description:
      then === "chooseFolder"
        ? "Run this in your own terminal, typing your Steam login where it asks. Then choose the folder it downloaded to."
        : "Run this in your own terminal, typing your Steam login where it asks: it downloads the files that need repair again. Then check the files.",
    body: [
      get === null
        ? null
        : h(
            "div",
            { class: "command-get" },
            h("p", {}, `Get DepotDownloader ${get.version} for Linux first, unzip it, and run the command in its folder:`),
            h("p", {}, button.link(get.url, () => void ctx.send({ type: "openUrl", url: get.url }))),
          ),
      h("div", { class: "command-line" }, code, copy),
    ],
    actions: [
      button.secondary("Close", close),
      then === "chooseFolder"
        ? button.primary("Choose folder", () => {
            void ctx.send({ type: "pickFolder", purpose: "game" });
            close();
          })
        : button.primary("Check files", () => {
            void ctx.send({ type: "check", level: "quick" });
            close();
          }),
    ],
    onClose: close,
  });
  return { el: shown.el, close };
}

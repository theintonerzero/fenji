// What the Client tab's screens share (the boards ClientSetup, Downloading and Repair): the hero
// they stand in, beside the blade, and the column's "Game client" card.
import { h, type Child } from "../dom";
import { activeServer } from "../route";
import type { Snapshot } from "../types";
import { facts } from "../ui/card";
import { cardSection } from "../ui/column";
import { people } from "../ui/people";
import { seen, versionLine } from "../words";
import { contextColumn } from "./parts";

export const GAME = "Swords of Legends Online";

/** A folder in Geist Mono, as DESIGN.md writes paths. */
export function path(text: string): HTMLElement {
  return h("span", { class: "mono path" }, text);
}

/** A path's last folders ("…/Games/SoLO") where a narrow place would cut it, the whole in its title. */
export function pathEnd(text: string, max: number): HTMLElement {
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
            ["Folder", s.client.folder === null ? "Not chosen yet" : path(s.client.folder)],
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

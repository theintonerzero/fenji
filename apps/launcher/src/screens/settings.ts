// Settings (launcher spec §3.5, §3.7, §3.8, §3.11; the Settings board), without an ember action:
// the game folder and "Check every file"; updates, with "Check now"; umu-run's path on Linux
// (empty means the default); the log folder; and About in the column. Each change is saved as it
// is made.
import type { Ctx, Screen } from "../app";
import { h, type Child } from "../dom";
import type { Snapshot } from "../types";
import { button } from "../ui/button";
import { facts } from "../ui/card";
import { icons } from "../ui/icons";
import { clientLine } from "../words";
import { PRIVACY } from "./account";
import { path } from "./client-parts";
import { contextColumn, noticeSlot, renew } from "./parts";

const REPOSITORY = "https://github.com/theintonerzero/fenji";
const LICENCE = `${REPOSITORY}/blob/main/LICENSE`;

function card(title: string, ...children: Child[]): HTMLElement {
  return h("section", { class: "card settings-card" }, h("h2", { class: "card-title" }, title), ...children);
}

/** The launcher's own version, then an update when the updater has found one, or "up to date" when
 * its last check found nothing newer (the Settings board). */
function version(s: Snapshot): Child[] {
  const { update } = s;
  if (update.checking) {
    return [update.current, h("span", { class: "muted" }, "· checking…")];
  }
  if (update.available === null) {
    return update.upToDate
      ? [update.current, h("span", { class: "dot dot-ok", "aria-hidden": "true" }), "up to date"]
      : [update.current];
  }
  return [
    update.current,
    h("span", { class: "dot dot-warn", "aria-hidden": "true" }),
    `${update.available} ${update.ready ? "ready" : "available"}`,
  ];
}

export function settings(root: HTMLElement, ctx: Ctx): Screen {
  const slot = noticeSlot(ctx);
  const folder = h("input", { id: "settings-folder", class: "input mono", type: "text", readonly: true, placeholder: "Not chosen yet" });
  const verified = h("p", { class: "client-fact settings-fact" });
  const auto = h("input", {
    class: "switch-input",
    type: "checkbox",
    role: "switch",
    id: "settings-auto-update",
    onchange: () => void ctx.send({ type: "setAutoUpdate", on: auto.checked }),
  });
  const umu = h("input", {
    id: "settings-umu-run",
    class: "input mono",
    type: "text",
    spellcheck: "false",
    onchange: () => {
      const typed = umu.value.trim();
      void ctx.send({ type: "setUmuRun", path: typed === "" ? null : typed });
    },
  });
  const logs = h("div", {});
  const linux = h("div", {});
  const side = contextColumn();
  const clientCard = card(
    "Client",
    h(
      "div",
      { class: "settings-label-row" },
      h("label", { class: "field-label", for: "settings-folder" }, "Game folder"),
      button.secondary("Change", () => void ctx.send({ type: "pickFolder", purpose: "game" }), "small"),
    ),
    folder,
    verified,
    h(
      "div",
      { class: "settings-check" },
      button.secondary("Check every file", () => void ctx.send({ type: "check", level: "full" }), "compact"),
      h("p", { class: "hint" }, "Bad files are repaired from your own Steam copy."),
    ),
  );
  const launcherCard = card(
    "Launcher",
    h(
      "label",
      { class: "switch-row", for: "settings-auto-update" },
      h(
        "span",
        {},
        h("span", { class: "switch-label" }, "Update automatically"),
        h("span", { class: "switch-hint" }, "Signed updates install when you restart."),
      ),
      h(
        "span",
        { class: "switch" },
        auto,
        h("span", { class: "switch-track", "aria-hidden": "true" }, h("span", { class: "switch-knob" })),
      ),
    ),
    h(
      "div",
      { class: "settings-check" },
      button.secondary("Check now", () => void ctx.send({ type: "checkUpdate" }), "small"),
    ),
    h("p", { class: "settings-language" }, "Language: English"),
  );
  root.append(
    h(
      "main",
      { class: "main page" },
      h(
        "div",
        { class: "title-row title-row-baseline" },
        h("h1", { class: "headline page-title" }, "Settings"),
        h("p", { class: "title-note" }, "Changes are saved on this computer as you make them."),
      ),
      slot.el,
      h(
        "div",
        { class: "settings-grid" },
        h("div", { class: "settings-column" }, clientCard, launcherCard),
        h("div", { class: "settings-column" }, linux, logs),
      ),
    ),
    side.el,
  );
  return {
    update(s) {
      slot.update(s);
      folder.value = s.client.folder ?? "";
      renew(verified, clientLine(s, true), () => {
        const ok = s.client.check.kind === "verified" ? icons.check(13) : null;
        ok?.classList.add("icon-ok");
        return [ok, clientLine(s, true)];
      });
      if (auto.checked !== s.settings.autoUpdate) {
        auto.checked = s.settings.autoUpdate;
      }
      // Linux only: umu-run starts the client there (spec §3.7, L2).
      renew(linux, String(s.platform.os === "linux"), () =>
        s.platform.os === "linux"
          ? [
              card(
                "Linux",
                h("p", { class: "settings-lede" }, "Fenji starts the client with umu-run and Proton."),
                h("label", { class: "field-label", for: "settings-umu-run" }, "umu-run"),
                umu,
                h("p", { class: "hint" }, "Leave it empty to use the one on your PATH, or Fenji's own copy."),
              ),
            ]
          : [],
      );
      if (document.activeElement !== umu) {
        umu.value = s.settings.umuRun ?? "";
      }
      renew(logs, s.settings.logFolder, () => [
        card(
          "Logs",
          h("p", { class: "settings-lede" }, "The launcher's and the game's logs, for when something goes wrong."),
          h("p", { class: "settings-path" }, path(s.settings.logFolder)),
          button.secondary("Open the log folder", () => void ctx.send({ type: "openLogFolder" }), "compact"),
        ),
      ]);
      side.show(
        JSON.stringify(s.update),
        () => {
          const shield = icons.shield(16);
          shield.classList.add("icon-ok");
          return [
            h(
              "section",
              {},
              h("h2", { class: "column-heading" }, "About"),
              facts([
                ["Launcher", h("span", { class: "about-version" }, ...version(s))],
                [
                  "Licence",
                  h(
                    "button",
                    { class: "inline-link", type: "button", onclick: () => void ctx.send({ type: "openUrl", url: LICENCE }) },
                    "AGPL-3.0-or-later",
                  ),
                ],
                ["Made by", "The Fenji contributors"],
              ]),
              button.link(["Source code", icons.external(13)], () => void ctx.send({ type: "openUrl", url: REPOSITORY })),
            ),
            h(
              "section",
              { class: "card column-card telemetry" },
              h("p", { class: "telemetry-title" }, shield, "No telemetry: Fenji sends nothing about you."),
              h(
                "p",
                { class: "telemetry-link" },
                h(
                  "button",
                  { class: "inline-link", type: "button", onclick: () => void ctx.send({ type: "openUrl", url: PRIVACY }) },
                  "Read the privacy page",
                ),
              ),
            ),
          ];
        },
        "Fenji is a private server for Swords of Legends Online, for friends you invite.",
      );
    },
    destroy() {
      root.replaceChildren();
    },
  };
}

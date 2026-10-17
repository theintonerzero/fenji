// The Settings popover (the redesign's Settings board): updates; on Linux GameMode, launch options,
// umu-run and the download folder; and where the logs, the licences, the source and the privacy
// page are. Each change is saved as it's made.
import type { Ctx, PopoverContent } from "../app";
import { h } from "../dom";
import type { Intent } from "../types";
import { button } from "../ui/button";
import { refused, renew, shown } from "../screens/parts";
import { field } from "../ui/field";
import { versionLine } from "../words";

const REPOSITORY = "https://github.com/theintonerzero/fenji";
/** PRIVACY.md as released: the main branch holds only tagged releases. */
export const PRIVACY = `${REPOSITORY}/blob/main/PRIVACY.md`;

/** A real checkbox under the switch (DESIGN.md, Switch), described by the line under its label. */
function switchInput(id: string, describedBy: string, onchange: (on: boolean) => void): HTMLInputElement {
  const input = h("input", {
    class: "switch-input",
    type: "checkbox",
    role: "switch",
    id,
    "aria-describedby": describedBy,
    onchange: () => onchange(input.checked),
  });
  return input;
}

/** A setting's row: its label over its line, and its switch. */
function switchRow(input: HTMLInputElement, label: string, line: HTMLElement): HTMLElement {
  return h(
    "div",
    { class: "switch-row" },
    h("div", {}, h("label", { class: "big", for: input.id }, label), line),
    h("span", { class: "switch" }, input, h("span", { class: "switch-track", "aria-hidden": "true" }, h("span", { class: "switch-knob" }))),
  );
}

export function settingsPopover(ctx: Ctx): PopoverContent {
  const auto = switchInput("settings-auto-update", "settings-version", (on) => void ctx.send({ type: "setAutoUpdate", on }));
  // GameMode, where it's installed (launcher spec Amendment B, LB2).
  const gamemode = switchInput("settings-gamemode", "settings-gamemode-line", (on) => void ctx.send({ type: "setGameMode", on }));
  const gamemodeRow = switchRow(
    gamemode,
    "GameMode",
    h("p", { class: "line", id: "settings-gamemode-line" }, "Performance mode while the game runs."),
  );
  // Launch options, as Steam's (launcher spec Amendment B, LB3). The model refuses what can't be
  // used, in words under the field, which keeps the player's text until it's changed.
  const optionsInput = h("input", {
    id: "settings-launch-options",
    class: "mono",
    type: "text",
    spellcheck: "false",
    autocomplete: "off",
    placeholder: "PROTON_LOG=1 mangohud %command%",
    onchange: () => void sendOptions(),
  });
  const options = field("Launch options", optionsInput);
  let optionsRefused = false;
  const sendOptions = async (): Promise<void> => {
    const refusal = await ctx.send({ type: "setLaunchOptions", text: optionsInput.value });
    optionsRefused = refusal !== null;
    if (refusal === null) {
      options.setError(null);
    } else {
      refused(ctx, null, refusal, (words) => options.setError(words));
    }
  };
  // One link whose words and intent change in place, so it keeps focus while a check runs (R7).
  const versionWords = h("span", {});
  let versionIntent: Intent = { type: "checkUpdate" };
  const versionAction = button.link("Check now", () => void ctx.send(versionIntent));
  const version = h("p", { class: "line", id: "settings-version" }, versionWords, " · ", versionAction);
  const umu = h("input", {
    id: "settings-umu-run",
    class: "input mono",
    type: "text",
    spellcheck: "false",
    placeholder: "Automatic",
    onchange: () => {
      const typed = umu.value.trim();
      void ctx.send({ type: "setUmuRun", path: typed === "" ? null : typed });
    },
  });
  const folder = h("button", {
    class: "input mono field-button",
    type: "button",
    id: "settings-download-folder",
    onclick: () => void ctx.send({ type: "pickFolder", purpose: "download" }),
  });
  const linux = h("div", {});
  const body = h(
    "div",
    { class: "sections" },
    h("section", { class: "sec" }, switchRow(auto, "Auto-update", version)),
    linux,
    h(
      "section",
      { class: "sec" },
      h(
        "div",
        { class: "acts links" },
        button.link("Logs", () => void ctx.send({ type: "openLogFolder" })),
        button.link("Licences", () => void ctx.send({ type: "openNotices" })),
        button.link("Source", () => void ctx.send({ type: "openUrl", url: REPOSITORY })),
        button.link("Privacy", () => void ctx.send({ type: "openUrl", url: PRIVACY })),
      ),
    ),
  );
  return {
    title: "Settings",
    body,
    update(s) {
      if (auto.checked !== s.settings.autoUpdate) {
        auto.checked = s.settings.autoUpdate;
      }
      versionWords.textContent = versionLine(s);
      // A ready update stays installable here, after Later too.
      const ready = s.update.ready;
      versionIntent = ready ? { type: "installUpdate" } : { type: "checkUpdate" };
      const words = ready ? "Restart" : "Check now";
      if (versionAction.textContent !== words) {
        versionAction.textContent = words;
      }
      // Linux only: umu-run starts the game there, and only there can Fenji download it. GameMode's
      // switch shows where it's installed.
      const installed = s.settings.gamemode !== null;
      renew(linux, JSON.stringify([s.platform.os, installed]), () =>
        s.platform.os === "linux"
          ? [
              ...(installed ? [h("section", { class: "sec" }, gamemodeRow)] : []),
              h("section", { class: "sec" }, options.el),
              h("section", { class: "sec" }, h("label", { class: "label", for: "settings-umu-run" }, "umu-run"), umu),
              h(
                "section",
                { class: "sec" },
                h("label", { class: "label", for: "settings-download-folder" }, "Download folder"),
                folder,
              ),
            ]
          : [],
      );
      if (s.settings.gamemode !== null && gamemode.checked !== s.settings.gamemode) {
        gamemode.checked = s.settings.gamemode;
      }
      if (document.activeElement !== optionsInput && !optionsRefused) {
        optionsInput.value = s.settings.launchOptions;
      }
      if (document.activeElement !== umu) {
        umu.value = s.settings.umuRun ?? "";
      }
      const where = shown(s.client.defaultFolder, s.platform.home);
      if (folder.textContent !== where) {
        folder.textContent = where;
        folder.setAttribute("aria-label", `Download folder: ${where}. Change`);
      }
    },
  };
}

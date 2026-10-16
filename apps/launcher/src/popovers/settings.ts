// The Settings popover (the redesign's Settings board): updates, umu-run and the download folder
// on Linux, and where the logs, the licences, the source and the privacy page are. Each change is
// saved as it's made.
import type { Ctx, PopoverContent } from "../app";
import { h } from "../dom";
import { button } from "../ui/button";
import { renew, shown } from "../screens/parts";
import { versionLine } from "../words";

const REPOSITORY = "https://github.com/theintonerzero/fenji";
/** PRIVACY.md as released: the main branch holds only tagged releases. */
export const PRIVACY = `${REPOSITORY}/blob/main/PRIVACY.md`;

export function settingsPopover(ctx: Ctx): PopoverContent {
  const auto = h("input", {
    class: "switch-input",
    type: "checkbox",
    role: "switch",
    id: "settings-auto-update",
    "aria-describedby": "settings-version",
    onchange: () => void ctx.send({ type: "setAutoUpdate", on: auto.checked }),
  });
  const version = h("p", { class: "line", id: "settings-version" });
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
    h(
      "section",
      { class: "sec" },
      h(
        "div",
        { class: "switch-row" },
        h("div", {}, h("label", { class: "big", for: "settings-auto-update" }, "Auto-update"), version),
        h("span", { class: "switch" }, auto, h("span", { class: "switch-track", "aria-hidden": "true" }, h("span", { class: "switch-knob" }))),
      ),
    ),
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
      const { update } = s;
      renew(version, JSON.stringify(update), () => [
        versionLine(s),
        " · ",
        // A ready update stays installable here, after Later too.
        update.ready
          ? button.link("Restart", () => void ctx.send({ type: "installUpdate" }))
          : button.link("Check now", () => void ctx.send({ type: "checkUpdate" })),
      ]);
      // Linux only: umu-run starts the game there, and only there can Fenji download it.
      renew(linux, String(s.platform.os === "linux"), () =>
        s.platform.os === "linux"
          ? [
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

import { expect, test } from "vitest";

import { golden, mountPopover, press } from "../screens/test-kit";
import { PRIVACY, settingsPopover } from "./settings";

test("updates, umu-run and the download folder on Linux, and the links", () => {
  const { root, sent } = mountPopover(settingsPopover, golden("settings"));
  const auto = root.querySelector<HTMLInputElement>("#settings-auto-update")!;
  expect(auto.checked).toBe(true);
  expect(root.querySelector("#settings-version")!.textContent).toBe("0.5.0 · up to date · Check now");
  auto.click();
  press(root, "Check now");
  const umu = root.querySelector<HTMLInputElement>("#settings-umu-run")!;
  expect(umu.placeholder).toBe("Automatic");
  umu.value = " /opt/umu/umu-run ";
  umu.dispatchEvent(new Event("change"));
  const folder = root.querySelector<HTMLButtonElement>("#settings-download-folder")!;
  expect(folder.textContent).toBe("~/Games/SoLO");
  folder.click();
  for (const words of ["Logs", "Licences", "Source", "Privacy"]) {
    press(root, words);
  }
  expect(sent).toEqual([
    { type: "setAutoUpdate", on: false },
    { type: "checkUpdate" },
    { type: "setUmuRun", path: "/opt/umu/umu-run" },
    { type: "pickFolder", purpose: "download" },
    { type: "openLogFolder" },
    { type: "openNotices" },
    { type: "openUrl", url: "https://github.com/theintonerzero/fenji" },
    { type: "openUrl", url: PRIVACY },
  ]);
});

test("a ready update stays installable here", () => {
  const { root, sent } = mountPopover(settingsPopover, golden("update-ready"));
  expect(root.querySelector("#settings-version")!.textContent).toBe("0.5.0 · 0.5.1 ready · Restart");
  press(root, "Restart");
  expect(sent).toEqual([{ type: "installUpdate" }]);
});

test("off Linux there's no umu-run and no download folder", () => {
  const { root } = mountPopover(settingsPopover, golden("play-unsupported"));
  expect(root.querySelector("#settings-umu-run")).toBeNull();
  expect(root.querySelector("#settings-download-folder")).toBeNull();
});

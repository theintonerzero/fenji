import { expect, test } from "vitest";

import { settings } from "./settings";
import { embers, golden, mount, press } from "./test-kit";

test("no ember action; the switch, Check now and the log folder send their intents", () => {
  const { root, sent } = mount(settings, golden("settings"), undefined, "settings");
  expect(embers(root)).toBe(0);
  const toggle = root.querySelector<HTMLInputElement>('[role="switch"]')!;
  expect(toggle.checked).toBe(true);
  toggle.click();
  press(root, "Check now");
  press(root, "Open the log folder");
  expect(sent).toEqual([{ type: "setAutoUpdate", on: false }, { type: "checkUpdate" }, { type: "openLogFolder" }]);
});

test("the client card: the game folder, Change and Check every file", () => {
  const { root, sent } = mount(settings, golden("settings"), undefined, "settings");
  expect(root.querySelector<HTMLInputElement>("#settings-folder")!.value).toBe("/home/player/Games/SoLO");
  expect(root.textContent).toContain("Verified · build 10015727 · 210 files");
  press(root, "Change");
  press(root, "Check every file");
  expect(sent).toEqual([
    { type: "pickFolder", purpose: "game" },
    { type: "check", level: "full" },
  ]);
});

test("umu-run's path: typed, or empty for the default", () => {
  const { root, sent } = mount(settings, golden("settings"), undefined, "settings");
  const path = root.querySelector<HTMLInputElement>("#settings-umu-run")!;
  path.value = "/usr/bin/umu-run";
  path.dispatchEvent(new Event("change"));
  path.value = "  ";
  path.dispatchEvent(new Event("change"));
  expect(sent).toEqual([
    { type: "setUmuRun", path: "/usr/bin/umu-run" },
    { type: "setUmuRun", path: null },
  ]);
});

test("About: the version, the licence, the contributors, no telemetry, and two links", () => {
  const { root, sent } = mount(settings, golden("settings"), undefined, "settings");
  for (const words of ["0.5.0", "AGPL-3.0-or-later", "The Fenji contributors", "No telemetry: Fenji sends nothing about you.", "Language: English"]) {
    expect(root.textContent).toContain(words);
  }
  press(root, "Read the privacy page");
  press(root, "Source code");
  expect(sent).toEqual([
    { type: "openUrl", url: "https://github.com/theintonerzero/fenji/blob/main/PRIVACY.md" },
    { type: "openUrl", url: "https://github.com/theintonerzero/fenji" },
  ]);
});

test("off Linux there is no umu-run card", () => {
  const s = golden("settings");
  s.platform = { os: "windows", canPlay: false, canDownload: false };
  const { root } = mount(settings, s, undefined, "settings");
  expect(root.querySelector("#settings-umu-run")).toBeNull();
});

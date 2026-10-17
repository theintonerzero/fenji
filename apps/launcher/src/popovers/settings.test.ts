import { expect, test } from "vitest";

import { golden, mountPopover, press, settle } from "../screens/test-kit";
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
  expect(root.querySelector("#settings-gamemode")).toBeNull();
  expect(root.querySelector("#settings-launch-options")).toBeNull();
});

test("launch options: sent as typed, a refusal in words under the field, which keeps the text", async () => {
  const s = golden("settings");
  const refusal = "The launch options have %command% twice: keep one.";
  const { root, sent, content } = mountPopover(settingsPopover, s, (intent) =>
    intent.type === "setLaunchOptions" && intent.text.split("%command%").length > 2 ? refusal : null,
  );
  const input = root.querySelector<HTMLInputElement>("#settings-launch-options")!;
  expect(root.querySelector('label[for="settings-launch-options"]')!.textContent).toBe("Launch options");
  expect(input.placeholder).toBe("PROTON_LOG=1 mangohud %command%");
  expect(input.classList.contains("mono")).toBe(true);
  // Refused: the words under the field, the text kept through the model's next snapshot.
  input.value = "%command% -x %command%";
  input.dispatchEvent(new Event("change"));
  await settle();
  const error = root.querySelector(`#${input.id}-error`)!;
  expect((error as HTMLElement).hidden).toBe(false);
  expect(error.textContent).toBe(refusal);
  expect(input.getAttribute("aria-invalid")).toBe("true");
  content.update(s);
  expect(input.value).toBe("%command% -x %command%");
  // Accepted: the error goes, and the model's text shows.
  input.value = "mangohud %command%";
  input.dispatchEvent(new Event("change"));
  await settle();
  expect((error as HTMLElement).hidden).toBe(true);
  expect(input.hasAttribute("aria-invalid")).toBe(false);
  content.update({ ...s, settings: { ...s.settings, launchOptions: "mangohud %command%" } });
  expect(input.value).toBe("mangohud %command%");
  expect(sent).toEqual([
    { type: "setLaunchOptions", text: "%command% -x %command%" },
    { type: "dismissNotice", text: refusal },
    { type: "setLaunchOptions", text: "mangohud %command%" },
  ]);
});

test("GameMode's switch shows on Linux where GameMode is installed, and follows the model", () => {
  const s = golden("settings");
  const { root, sent, content } = mountPopover(settingsPopover, s);
  const gamemode = root.querySelector<HTMLInputElement>("#settings-gamemode")!;
  expect(gamemode.getAttribute("role")).toBe("switch");
  expect(gamemode.checked).toBe(true);
  expect(root.querySelector('label[for="settings-gamemode"]')!.textContent).toBe("GameMode");
  const line = root.querySelector(`#${gamemode.getAttribute("aria-describedby")}`)!;
  expect(line.textContent).toBe("Performance mode while the game runs.");
  gamemode.click();
  expect(sent).toEqual([{ type: "setGameMode", on: false }]);
  content.update({ ...s, settings: { ...s.settings, gamemode: false } });
  expect(root.querySelector<HTMLInputElement>("#settings-gamemode")!.checked).toBe(false);
  // Without GameMode, no switch.
  content.update({ ...s, settings: { ...s.settings, gamemode: null } });
  expect(root.querySelector("#settings-gamemode")).toBeNull();
  expect(root.querySelector("#settings-umu-run")).not.toBeNull();
});

test("Check now keeps focus while the check runs (R7)", () => {
  const { root, content } = mountPopover(settingsPopover, golden("settings"));
  const link = [...root.querySelectorAll("button")].find((b) => b.textContent === "Check now")!;
  link.focus();
  const checking = golden("settings");
  checking.update.checking = true;
  content.update(checking);
  expect(document.activeElement).toBe(link);
  expect(root.querySelector("#settings-version")!.textContent).toBe("0.5.0 · checking… · Check now");
});

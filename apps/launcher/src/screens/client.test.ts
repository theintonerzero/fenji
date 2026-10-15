import { afterEach, expect, test, vi } from "vitest";

import { pathEnd } from "./client-parts";
import { clientReady } from "./client-ready";
import { clientRepair } from "./client-repair";
import { clientSetup } from "./client-setup";
import { clientWorking } from "./client-working";
import { embers, golden, mount, press } from "./test-kit";

afterEach(() => {
  vi.unstubAllGlobals();
  vi.useRealTimers();
});

test("setup: a folder, or a download into the default folder, which is the one ember action", () => {
  const { root, sent } = mount(clientSetup, golden("client-setup"), undefined, "client");
  // The card is narrow: the folder's end shows, and the whole path is its title.
  const folder = root.querySelector<HTMLElement>(".choice-folder .path")!;
  expect(folder.textContent).toBe("…/Games/SoLO");
  expect(folder.title).toBe("/home/player/Games/SoLO");
  expect(embers(root)).toBe(1);
  expect(root.querySelector(".ember")!.textContent).toBe("Start download");
  press(root, "Choose folder");
  press(root, "Change");
  press(root, "Start download");
  expect(sent).toEqual([
    { type: "pickFolder", purpose: "game" },
    { type: "pickFolder", purpose: "download" },
    { type: "download", folder: "/home/player/Games/SoLO" },
  ]);
});

test("setup: the manual command opens in a dialog, in mono, with Copy and Choose folder", async () => {
  const written: string[] = [];
  vi.stubGlobal("navigator", { clipboard: { writeText: async (text: string) => void written.push(text) } });
  const s = golden("client-setup");
  const { root, sent } = mount(clientSetup, s, undefined, "client");
  expect(document.querySelector('[role="dialog"]')).toBeNull();
  press(root, "No Steam app? Run DepotDownloader yourself");
  const shown = document.querySelector<HTMLElement>('[role="dialog"]')!;
  expect(shown.querySelector(".command-line code")!.textContent).toBe(s.client.manualCommand);
  press(shown, "Copy");
  await Promise.resolve();
  expect(written).toEqual([s.client.manualCommand]);
  press(shown, "Choose folder");
  expect(sent).toEqual([{ type: "pickFolder", purpose: "game" }]);
  expect(document.querySelector('[role="dialog"]')).toBeNull();
});

test("a path too long for its place shows its last folders, the whole in its title", () => {
  expect(pathEnd("/home/player/Games/SoLO", 18).textContent).toBe("…/Games/SoLO");
  expect(pathEnd("/home/player/Games/SoLO", 18).title).toBe("/home/player/Games/SoLO");
  expect(pathEnd("~/Games/SoLO", 18).textContent).toBe("~/Games/SoLO");
  expect(pathEnd("C:\\Users\\player\\Games\\SoLO", 16).textContent).toBe("…\\Games\\SoLO");
  expect(pathEnd("/a-very-long-folder-name", 8).textContent).toBe("…/a-very-long-folder-name");
});

test("setup off Linux: the Steam card says why and has no button", () => {
  const s = golden("client-setup");
  s.platform = { os: "windows", canPlay: false, canDownload: false };
  const { root } = mount(clientSetup, s, undefined, "client");
  expect(root.textContent).toContain("Downloading with Steam works on Linux only for now.");
  expect(root.textContent).not.toContain("Start download");
  expect(root.querySelector(".ember")!.textContent).toBe("Choose folder");
});

test("working: fetching the tool, waiting for Steam, and downloading, each in words", () => {
  const fetching = mount(clientWorking, golden("client-fetching"), undefined, "client").root;
  expect(fetching.textContent).toContain("Getting DepotDownloader…");
  expect(fetching.textContent).toContain("40%");
  const waiting = mount(clientWorking, golden("client-waiting-for-steam"), undefined, "client").root;
  expect(waiting.textContent).toContain("Sign in to Steam in DepotDownloader's window: scan its QR code with the Steam app.");
  const { root, sent } = mount(clientWorking, golden("client-downloading"), undefined, "client");
  expect(root.querySelector('[role="progressbar"]')!.getAttribute("aria-valuenow")).toBe("42");
  expect(root.textContent).toContain("data/_index/sc_res.idx");
  press(root, "Cancel download");
  expect(sent).toEqual([{ type: "cancelDownload" }]);
});

test("repair: the counts and each file's problem, Repair from Steam, and the Gameforge line", () => {
  const { root, sent } = mount(clientRepair, golden("client-repair"), undefined, "client");
  expect(root.textContent).toContain("209 files fine");
  expect(root.textContent).toContain("1 needs repair");
  const row = root.querySelector("tbody tr")!;
  expect(row.textContent).toContain("data/_index/sc_res.idx");
  expect(row.textContent).toContain("Wrong size");
  expect(embers(root)).toBe(1);
  press(root, "Repair from Steam");
  press(root, "Choose another folder");
  press(root, "check again");
  expect(sent).toEqual([
    { type: "repair" },
    { type: "pickFolder", purpose: "game" },
    { type: "check", level: "full" },
  ]);
});

test("repair names each kind of problem", () => {
  const s = golden("client-repair");
  if (s.client.check.kind === "needsRepair") {
    s.client.check.files = [
      { path: "a.pak", problem: "missing" },
      { path: "b.pak", problem: "wrongSize" },
      { path: "c.pak", problem: "wrongHash" },
    ];
  }
  const { root } = mount(clientRepair, s, undefined, "client");
  const problems = [...root.querySelectorAll("tbody tr")].map((r) => r.querySelector(".problem")!.textContent);
  expect(problems).toEqual(["Missing", "Wrong size", "Doesn't match build 10015727"]);
  expect(root.textContent).toContain("3 need repair");
});

test("repair off Linux has no Repair from Steam", () => {
  const s = golden("client-repair");
  s.platform = { os: "macos", canPlay: false, canDownload: false };
  const { root } = mount(clientRepair, s, undefined, "client");
  expect(root.textContent).not.toContain("Repair from Steam");
  expect(root.textContent).toContain("check again");
});

test("ready: the folder, the build and the last check, Check every file and Change folder", () => {
  // The boards' clock: 20:14 in Melbourne is 09:14 UTC, and the check ran at 09:10.
  vi.useFakeTimers({ now: 1792055640000, toFake: ["Date"] });
  const { root, sent } = mount(clientReady, golden("client-verified"), undefined, "client");
  expect(root.textContent).toContain("/home/player/Games/SoLO");
  expect(root.textContent).toContain("Verified · build 10015727 · 210 files");
  expect(root.textContent).toContain("Every file checked today at 09:10");
  press(root, "Check every file");
  press(root, "Change folder");
  expect(sent).toEqual([
    { type: "check", level: "full" },
    { type: "pickFolder", purpose: "game" },
  ]);
});

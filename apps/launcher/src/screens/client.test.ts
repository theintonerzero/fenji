import { afterEach, expect, test, vi } from "vitest";

import { pathEnd, shown } from "./client-parts";
import { clientReady } from "./client-ready";
import { clientRepair } from "./client-repair";
import { clientSetup } from "./client-setup";
import { clientWorking } from "./client-working";
import { embers, golden, mount, press } from "./test-kit";

afterEach(() => {
  vi.unstubAllGlobals();
  vi.useRealTimers();
});

test("setup: a folder, the one ember action as on the ClientSetup board, or a download into the default folder", () => {
  const { root, sent } = mount(clientSetup, golden("client-setup"), undefined, "client");
  // The home folder shows as ~, as on the board; a path still too long for the narrow card shows
  // its end, with the whole path as its title (pathEnd's own test, below).
  const folder = root.querySelector<HTMLElement>(".choice-folder .path")!;
  expect(folder.textContent).toBe("~/Games/SoLO");
  expect(embers(root)).toBe(1);
  // The board's emphasis (the finish review's fix 2): most invited players have the game already.
  expect(root.querySelector(".ember")!.textContent).toBe("Choose folder");
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
  const manual = s.client.manual!;
  expect(shown.querySelector(".command-line code")!.textContent).toBe(manual.download);
  press(shown, "Copy");
  await Promise.resolve();
  expect(written).toEqual([manual.download]);
  // Without Fenji's own DepotDownloader, the dialog says where to get it, the link on its own
  // (#348's final review: a first line of the command said it, as a comment).
  const get = manual.get!;
  expect(shown.textContent).toContain(`Get DepotDownloader ${get.version} for Linux`);
  press(shown, get.url);
  press(shown, "Choose folder");
  expect(sent).toEqual([
    { type: "openUrl", url: get.url },
    { type: "pickFolder", purpose: "game" },
  ]);
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
  s.platform = { os: "windows", canPlay: false, canDownload: false, home: null };
  const { root } = mount(clientSetup, s, undefined, "client");
  expect(root.textContent).toContain("Downloading with Steam works on Linux only for now.");
  expect(root.textContent).not.toContain("Start download");
  expect(root.querySelector(".ember")!.textContent).toBe("Choose folder");
});

test("working: fetching the tool, waiting for Steam, and downloading, each in words", () => {
  const fetching = mount(clientWorking, golden("client-fetching"), undefined, "client").root;
  expect(fetching.textContent).toContain("Getting DepotDownloader…");
  expect(fetching.textContent).toContain("40%");
  // The blade shows the player's copy (DESIGN.md, The Blade And Ground Rule), not the tool's
  // download: it stays unlit while DepotDownloader comes (the finish review's fix 3).
  expect(fetching.querySelector('[role="progressbar"]')).toBeNull();
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
  // A quick check, which hashes the files that needed repair in full (#348).
  expect(sent).toEqual([
    { type: "repair" },
    { type: "pickFolder", purpose: "game" },
    { type: "check", level: "quick" },
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

test("repair has a way without a terminal: its own command, then a check of the files (#348)", () => {
  const s = golden("client-repair");
  const { root, sent } = mount(clientRepair, s, undefined, "client");
  press(root, "No Steam app? Run DepotDownloader yourself");
  const shown = document.querySelector<HTMLElement>('[role="dialog"]')!;
  const repair = s.client.manual!.repair!;
  expect(repair).toContain(" -validate");
  expect(shown.querySelector(".command-line code")!.textContent).toBe(repair);
  press(shown, "Check files");
  expect(sent).toEqual([{ type: "check", level: "quick" }]);
  expect(document.querySelector('[role="dialog"]')).toBeNull();
});

test("repair offers no command for the player's terminal without its list of files", () => {
  const s = golden("client-repair");
  s.client.manual!.repair = null;
  const { root } = mount(clientRepair, s, undefined, "client");
  expect(root.textContent).not.toContain("No Steam app?");
});

test("setup leads with a copy Steam installed: Use it, there (Amendment A, LA1)", () => {
  const s = golden("client-setup");
  const folder = "/home/player/.local/share/Steam/steamapps/common/Swords of Legends Online";
  s.client.steam = { folder, build: "10015727", pinned: true };
  const { root, sent } = mount(clientSetup, s, undefined, "client");
  expect(root.textContent).toContain("Found in your Steam library");
  expect(root.textContent).toContain("~/.local/share/Steam/");
  expect(embers(root)).toBe(1);
  expect(root.querySelector(".ember")!.textContent).toBe("Use it");
  press(root, "Use it");
  press(root, "Choose another folder");
  expect(sent).toEqual([
    { type: "chooseFolder", path: folder },
    { type: "pickFolder", purpose: "game" },
  ]);
});

test("setup says when Steam's copy is another build, and offers no Use it", () => {
  const s = golden("client-setup");
  s.client.steam = { folder: "/home/player/.local/share/Steam/steamapps/common/SoLO", build: "9999999", pinned: false };
  const { root } = mount(clientSetup, s, undefined, "client");
  expect(root.textContent).toContain("Steam has build 9999999");
  expect(root.textContent).toContain("update the game in Steam");
  expect([...root.querySelectorAll("button")].map((b) => b.textContent)).not.toContain("Use it");
  expect(root.querySelector(".ember")!.textContent).toBe("Choose folder");
});

test("repair of a copy in a Steam library names Steam's own Verify, then checks the files (LA2)", () => {
  const s = golden("client-repair");
  s.client.inSteam = true;
  s.client.manual!.repair = null;
  const { root, sent } = mount(clientRepair, s, undefined, "client");
  expect(root.textContent).toContain("Verify integrity of game files");
  expect(root.textContent).not.toContain("Repair from Steam");
  expect(root.textContent).not.toContain("Gameforge");
  expect(embers(root)).toBe(1);
  press(root, "Check files");
  expect(sent).toEqual([{ type: "check", level: "quick" }]);
});

test("repair off Linux has no Repair from Steam", () => {
  const s = golden("client-repair");
  s.platform = { os: "macos", canPlay: false, canDownload: false, home: "/Users/player" };
  const { root } = mount(clientRepair, s, undefined, "client");
  expect(root.textContent).not.toContain("Repair from Steam");
  expect(root.textContent).toContain("check again");
});

test("ready: the folder, the build and the last check, Check every file and Change folder", () => {
  // The boards' clock: 20:14 in Melbourne is 09:14 UTC, and the check ran at 09:10.
  vi.useFakeTimers({ now: 1792055640000, toFake: ["Date"] });
  const { root, sent, tabs } = mount(clientReady, golden("client-verified"), undefined, "client");
  // The home folder shows as ~, as on the boards, so a screenshot doesn't carry the player's name.
  expect(root.textContent).toContain("~/Games/SoLO");
  expect(root.textContent).not.toContain("/home/player");
  // A ready copy leads on to Play (the finish review's minor), in gold, not a second ember.
  press(root, "Go to Play");
  expect(tabs).toEqual(["play"]);
  expect(root.textContent).toContain("Verified · build 10015727 · 210 files");
  expect(root.textContent).toContain("Every file checked today at 09:10");
  press(root, "Check every file");
  press(root, "Change folder");
  expect(sent).toEqual([
    { type: "check", level: "full" },
    { type: "pickFolder", purpose: "game" },
  ]);
});

test("a path shows the home folder as ~, and nothing that only starts with its name", () => {
  expect(shown("/home/player/Games/SoLO", "/home/player")).toBe("~/Games/SoLO");
  expect(shown("/home/player", "/home/player")).toBe("~");
  expect(shown("/home/playerx/Games", "/home/player")).toBe("/home/playerx/Games");
  expect(shown("/opt/SoLO", "/home/player")).toBe("/opt/SoLO");
  expect(shown("/home/player/Games", null)).toBe("/home/player/Games");
});

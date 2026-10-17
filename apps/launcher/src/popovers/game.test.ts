import { expect, test, vi } from "vitest";

import { embers, golden, mountPopover, press, settle, shows } from "../screens/test-kit";
import { gamePopover } from "./game";

test("verified: the game, where it is, Verified and when, Verify integrity and Change folder", () => {
  const { root, sent } = mountPopover(gamePopover, golden("client-verified"));
  expect(root.querySelector(".big")!.textContent).toBe("Swords of Legends Online");
  expect(root.querySelector(".path")!.textContent).toBe("~/Games/SoLO");
  expect(root.textContent).toContain("Verified");
  expect(embers(root)).toBe(0);
  press(root, "Verify integrity");
  press(root, "Change folder");
  expect(sent).toEqual([
    { type: "check", level: "full" },
    { type: "pickFolder", purpose: "game" },
  ]);
});

test("set up: another folder or a download, and Run it yourself for a player without a terminal Fenji can open", () => {
  const { root, sent, asked } = mountPopover(gamePopover, golden("client-setup"));
  expect([...root.querySelectorAll(".big")].map((b) => b.textContent)).toEqual(["Another folder", "Download"]);
  // Without a copy Steam installed, Choose folder is the one ember.
  expect(embers(root)).toBe(1);
  expect(root.querySelector(".ember")!.textContent).toBe("Choose folder");
  press(root, "Download");
  expect(sent).toEqual([{ type: "download", folder: "/home/player/Games/SoLO" }]);
  press(root, "Run it yourself");
  expect(asked.dialogs).toHaveLength(1);
  expect(document.body.textContent).toContain("./DepotDownloader -app 1418100");
});

test("set up: a copy Steam installed comes first, and Use it is the ember", () => {
  const s = golden("client-setup");
  s.client.steam = { folder: "/home/player/.local/share/Steam/steamapps/common/Swords of Legends Online", build: "10015727", pinned: true };
  const { root, sent } = mountPopover(gamePopover, s);
  expect([...root.querySelectorAll(".big")].map((b) => b.textContent)).toEqual(["Steam library", "Another folder", "Download"]);
  expect(embers(root)).toBe(1);
  expect(root.querySelector(".path")!.textContent).toMatch(/^~\/…\/.*Swords of Legends Online$/);
  press(root, "Use it");
  expect(sent).toEqual([{ type: "chooseFolder", path: s.client.steam.folder }]);
});

test("set up: Steam's copy at another build says to update it there first", () => {
  const s = golden("client-setup");
  s.client.steam = { folder: "/home/player/.local/share/Steam/steamapps/common/Swords of Legends Online", build: "1", pinned: false };
  const { root } = mountPopover(gamePopover, s);
  expect(root.textContent).toContain("It's another build. Update it in Steam first.");
  expect(shows(root, "Use it")).toBe(false);
  expect(root.querySelector(".ember")!.textContent).toBe("Choose folder");
});

test("downloading: its progress and Cancel, which keeps focus as the progress streams", () => {
  const { root, sent, content } = mountPopover(gamePopover, golden("client-downloading"));
  expect(root.querySelector(".work-head")!.textContent).toBe("Downloading42%");
  const bar = root.querySelector('[role="progressbar"]')!;
  expect(bar.getAttribute("aria-valuenow")).toBe("42");
  const cancel = [...root.querySelectorAll("button")].find((b) => b.textContent === "Cancel")!;
  cancel.focus();
  const next = golden("client-downloading");
  if (next.job?.kind === "downloading") {
    next.job.percent = 57;
  }
  content.update(next);
  expect(document.activeElement).toBe(cancel);
  expect(bar.getAttribute("aria-valuenow")).toBe("57");
  cancel.click();
  expect(sent).toEqual([{ type: "cancelDownload" }]);
});

/** How many modules a drawn code's path paints: each row's runs, `M x y h n v1 h-n z`. */
function painted(code: Element): number {
  const d = code.querySelector("path")!.getAttribute("d") ?? "";
  return [...d.matchAll(/h(\d+)v1/g)].reduce((sum, run) => sum + Number(run[1]), 0);
}

function codeOf(s: ReturnType<typeof golden>): { size: number; dark: string } {
  if (s.job?.kind !== "downloading" || s.job.code === null) {
    throw new Error("no code in this snapshot");
  }
  return s.job.code;
}

test("signing in by code: the code drawn as squares, its words, Cancel and a password sign-in", () => {
  const s = golden("client-scan-code");
  const { root, sent, asked } = mountPopover(gamePopover, s);
  const code = root.querySelector('svg[role="img"]')!;
  expect(code.getAttribute("aria-label")).toBe("Steam sign-in code");
  expect(code.getAttribute("viewBox")).toBe(`0 0 ${codeOf(s).size} ${codeOf(s).size}`);
  // Every dark module is painted, and nothing else.
  expect(painted(code)).toBe([...codeOf(s).dark].filter((m) => m === "1").length);
  expect(root.textContent).toContain("Scan this with the Steam Mobile App.");
  expect(root.textContent).toContain("Fenji never sees your sign-in.");
  // The code is the next step, not progress: no bar, and no ember beside it.
  expect((root.querySelector('[role="progressbar"]') as HTMLElement).hidden).toBe(true);
  expect(embers(root)).toBe(0);
  expect(shows(root, "Run it yourself")).toBe(false);
  expect(root.textContent).toContain("No Steam Mobile App? Sign in with your password");
  // A password sign-in is the player's own command, with their own login, in their own terminal
  // (L3, LA5; the final review, C2): Fenji's run, waiting, ends first.
  press(root, "Sign in with your password");
  expect(sent).toEqual([{ type: "cancelDownload" }]);
  expect(asked.dialogs).toHaveLength(1);
  expect(document.body.textContent).toContain("Then choose the folder it downloaded to.");
  press(root, "Cancel");
  expect(sent).toEqual([{ type: "cancelDownload" }, { type: "cancelDownload" }]);
});

test("a repair waiting for its sign-in: the password route runs the repair's own command", () => {
  const s = golden("client-scan-code");
  s.client.manual = golden("client-repair").client.manual;
  if (s.job?.kind === "downloading") {
    s.job.repair = true;
  }
  const { root, sent, asked } = mountPopover(gamePopover, s);
  press(root, "Sign in with your password");
  expect(sent).toEqual([{ type: "cancelDownload" }]);
  expect(asked.dialogs).toHaveLength(1);
  expect(document.body.textContent).toContain("It downloads the files that need repair. Then verify them.");
});

test("a changed code is drawn again in its place, and the same one isn't", () => {
  const s = golden("client-scan-code");
  const { root, content } = mountPopover(gamePopover, s);
  const drawn = root.querySelector('svg[role="img"]')!;
  const before = drawn.querySelector("path")!.getAttribute("d");
  content.update(golden("client-scan-code"));
  expect(root.querySelector('svg[role="img"]')).toBe(drawn);
  const next = golden("client-scan-code");
  const { size, dark } = codeOf(next);
  const flipped = [...dark].map((m, i) => (i === size * 10 + 10 ? (m === "1" ? "0" : "1") : m)).join("");
  if (next.job?.kind === "downloading") {
    next.job.code = { size, dark: flipped };
  }
  content.update(next);
  const after = root.querySelector('svg[role="img"] path')!.getAttribute("d");
  expect(after).not.toBe(before);
  expect(painted(root.querySelector('svg[role="img"]')!)).toBe([...flipped].filter((m) => m === "1").length);
});

test("before Steam draws a code: connecting, the bar drifting, Cancel and a password sign-in", () => {
  const s = golden("client-scan-code");
  if (s.job?.kind === "downloading") {
    s.job.code = null;
  }
  const { root, sent, asked } = mountPopover(gamePopover, s);
  expect(root.querySelector('svg[role="img"]')).toBeNull();
  expect(root.textContent).toContain("Connecting to Steam…");
  expect((root.querySelector('[role="progressbar"]') as HTMLElement).hidden).toBe(false);
  press(root, "Sign in with your password");
  expect(sent).toEqual([{ type: "cancelDownload" }]);
  expect(asked.dialogs).toHaveLength(1);
});

test("while files come in, Cancel stands alone", () => {
  const s = golden("client-downloading");
  s.client.manual = golden("client-waiting-for-steam").client.manual;
  const { root } = mountPopover(gamePopover, s);
  expect(shows(root, "Run it yourself")).toBe(false);
  expect(shows(root, "Sign in with your password")).toBe(false);
  expect(shows(root, "Cancel")).toBe(true);
});

test("verifying: its progress and Stop", () => {
  const { root, sent } = mountPopover(gamePopover, golden("play-checking"));
  expect(root.querySelector(".work-head")!.textContent).toBe("Verifying78%");
  press(root, "Stop");
  expect(sent).toEqual([{ type: "stopCheck" }]);
});

test("needs repair: each file and why, Repair the one ember, and its refusal shows here", async () => {
  const { root, sent } = mountPopover(gamePopover, golden("client-repair"), (i) => (i.type === "repair" ? "Something's already running. Wait, or stop it." : null));
  expect(root.querySelector(".big")!.textContent).toBe("1 file needs repair");
  expect(root.querySelector(".file")!.textContent).toBe("data/_index/sc_res.idxWrong size");
  expect(embers(root)).toBe(1);
  // One row, as the board has it: Repair, Change folder, and the player's own terminal.
  const row = root.querySelectorAll(".sec .acts");
  expect(row).toHaveLength(1);
  expect([...row[0]!.querySelectorAll("button")].map((b) => b.textContent)).toEqual(["Repair", "Change folder", "Run it yourself"]);
  press(root, "Repair");
  await settle();
  expect(root.querySelector(".form-error")!.textContent).toBe("Something's already running. Wait, or stop it.");
  expect(sent).toEqual([
    { type: "repair" },
    { type: "dismissNotice", text: "Something's already running. Wait, or stop it." },
  ]);
});

test("needs repair in a Steam library: Steam repairs it, then Verify", () => {
  const s = golden("client-repair");
  s.client.inSteam = true;
  const { root, sent } = mountPopover(gamePopover, s);
  expect(root.textContent).toContain("Steam looks after this copy");
  expect(shows(root, "Repair")).toBe(false);
  expect(shows(root, "Run it yourself")).toBe(false);
  press(root, "Verify");
  expect(sent).toEqual([{ type: "check", level: "quick" }]);
});

test("a probe a minute later leaves a focused Verify integrity where it is (R7)", () => {
  vi.useFakeTimers();
  vi.setSystemTime(new Date(2026, 9, 16, 21, 4, 50));
  try {
    const { root, content } = mountPopover(gamePopover, golden("client-verified"));
    const verify = [...root.querySelectorAll("button")].find((b) => b.textContent === "Verify integrity")!;
    verify.focus();
    vi.setSystemTime(new Date(2026, 9, 16, 21, 6, 10));
    content.update(golden("client-verified"));
    expect(document.activeElement).toBe(verify);
  } finally {
    vi.useRealTimers();
  }
});

test("repair offers a plain Verify too, for files put back another way (launcher spec §3.5)", () => {
  const { root, sent } = mountPopover(gamePopover, golden("client-repair"));
  expect(root.textContent).toContain("Put them back another way? Verify");
  press(root, "Verify");
  expect(sent).toEqual([{ type: "check", level: "quick" }]);
});


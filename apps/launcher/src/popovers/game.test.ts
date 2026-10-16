import { expect, test } from "vitest";

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

test("waiting for Steam: the sign-in stays in its own window, or the player runs it in their own", () => {
  const { root, asked } = mountPopover(gamePopover, golden("client-waiting-for-steam"));
  expect(root.textContent).toContain("Sign in to Steam in its own window. Fenji never sees it.");
  expect(root.querySelector('[role="progressbar"]')!.hasAttribute("aria-valuenow")).toBe(false);
  expect(shows(root, "Run it yourself")).toBe(true);
  press(root, "Run it yourself");
  expect(asked.dialogs).toHaveLength(1);
  expect(document.body.textContent).toContain("Then choose the folder it downloaded to.");
});

test("while files come in, Cancel stands alone", () => {
  const s = golden("client-downloading");
  s.client.manual = golden("client-waiting-for-steam").client.manual;
  const { root } = mountPopover(gamePopover, s);
  expect(shows(root, "Run it yourself")).toBe(false);
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

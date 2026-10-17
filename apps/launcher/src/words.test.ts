import { expect, test } from "vitest";

import { golden } from "./test-golden";
import { gameChip, playWords, seen, stateLine, versionLine, when } from "./words";

const NOW = Date.UTC(2026, 9, 15, 20, 14);
const server = (file: string) => golden(file).servers[0]!;

test("one line of state (R4): the ping while PLAY is lit, players as 3/10", () => {
  expect(stateLine(server("play-ready"), NOW, true)).toBe("Online · 3/10 players · 24 ms");
  expect(stateLine(server("play-ready"), NOW, false)).toBe("Online · 3/10 players");
  expect(stateLine(server("play-maintenance"), NOW, false)).toMatch(/^Maintenance · back around (\w+ )?\d\d:\d\d$/);
  expect(stateLine(server("play-offline"), NOW, false)).toMatch(/^Offline · last seen (today \d\d:\d\d|yesterday \d\d:\d\d|\d+ \w{3})$/);
  expect(stateLine(server("play-key-changed"), NOW, false)).toBe("Key changed");
});

test("times read as the boards write them", () => {
  const today = new Date(2026, 9, 15, 9, 10).getTime();
  const now = new Date(2026, 9, 15, 20, 14).getTime();
  const yesterday = new Date(2026, 9, 14, 23, 10).getTime();
  const earlier = new Date(2026, 9, 12, 8, 0).getTime();
  expect(seen(yesterday, now)).toBe("yesterday 23:10");
  expect(seen(earlier, now)).toBe("12 Oct");
  expect(when(today, now)).toBe("Today at 09:10");
  expect(when(yesterday, now)).toBe("Yesterday at 23:10");
});

test.each([
  ["client-verified", "Game ready", "ok"],
  ["client-setup", "Set up the game", "quiet"],
  ["client-repair", "1 file needs repair", "warn"],
  ["client-downloading", "Downloading 42%", "quiet"],
  // A sign-in waits on the player (the final review, I6).
  ["client-waiting-for-steam", "Sign in to Steam", "warn"],
  ["client-scan-code", "Sign in to Steam", "warn"],
  ["client-fetching", "Downloading", "quiet"],
  ["play-checking", "Verifying", "quiet"],
])("the Game chip on %s reads %s", (file, label, tone) => {
  expect(gameChip(golden(file))).toEqual({ label, tone });
});

test("no file counts or build numbers on PLAY (R5)", () => {
  expect(playWords({ kind: "kindle", task: "verifying", percent: 78.4 })).toBe("Verifying 78%");
  expect(playWords({ kind: "kindle", task: "starting", percent: 30 })).toBe("Starting");
  expect(playWords({ kind: "running" })).toBe("Running");
  expect(playWords({ kind: "closed" })).toBe("Play");
});

test("the version says up to date only after a check found nothing newer", () => {
  const s = golden("play-ready");
  expect(versionLine(s)).toBe("0.5.0 · up to date");
  s.update.upToDate = false;
  expect(versionLine(s)).toBe("0.5.0");
  expect(versionLine(golden("update-ready"))).toBe("0.5.0 · 0.5.1 ready");
});

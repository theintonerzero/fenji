import { expect, test } from "vitest";

import { groundFor, playLook, playVariant, route } from "./route";
import { golden } from "./test-golden";

test.each([
  ["welcome", "welcome"],
  ["redeem", "redeem"],
  ["sign-in", "signIn"],
  ["play-ready", "stage"],
  ["play-signed-out", "stage"],
  ["play-maintenance", "stage"],
  ["client-setup", "stage"],
  ["servers", "stage"],
])("%s shows %s", (file, screen) => {
  expect(route(golden(file))).toBe(screen);
});

test("a sign-in the player asked for shows on a server they're signed out of, and only there", () => {
  expect(route(golden("play-signed-out"), { signIn: true })).toBe("signIn");
  expect(route(golden("play-ready"), { signIn: true })).toBe("stage");
});

test("the invite waits on its own server: I have an account signs in there, and other seals lead to their stages", () => {
  expect(route(golden("redeem"), { signIn: true })).toBe("signIn");
  const elsewhere = golden("servers");
  elsewhere.pendingInvite = { serverId: 2, code: "K7QM-3XRD-VN2P-HT8B" };
  expect(route(elsewhere)).toBe("stage");
  elsewhere.active = 2;
  expect(route(elsewhere)).toBe("redeem");
});

test.each([
  ["play-ready", "ready"],
  ["play-signed-out", "ready"],
  ["play-checking", "checking"],
  ["play-starting", "starting"],
  ["play-running", "running"],
  ["play-maintenance", "maintenance"],
  ["play-offline", "offline"],
  ["play-key-changed", "blocked"],
  ["play-unsupported", "unsupported"],
  ["client-setup", "notReady"],
])("%s plays as %s", (file, variant) => {
  expect(playVariant(golden(file))).toBe(variant);
});

test("PLAY kindles: lit when ready, filled to a check's progress, closed when Play can't happen, an outline running", () => {
  expect(playLook(golden("play-ready"))).toEqual({ kind: "lit" });
  expect(playLook(golden("play-checking"))).toEqual({ kind: "kindle", task: "verifying", percent: 78 });
  expect(playLook(golden("play-starting"))).toEqual({ kind: "kindle", task: "starting", percent: null });
  expect(playLook(golden("play-running"))).toEqual({ kind: "running" });
  expect(playLook(golden("play-maintenance"))).toEqual({ kind: "closed" });
  expect(playLook(golden("client-setup"))).toEqual({ kind: "closed" });
  // A first start's umu-launcher download is the only progress a start has.
  const fetching = golden("play-starting");
  fetching.job = { kind: "fetching", tool: "umu-launcher", percent: 30 };
  expect(playLook(fetching)).toEqual({ kind: "kindle", task: "starting", percent: 30 });
});

test.each([
  ["play-ready", "stage", "warm"],
  ["play-running", "stage", "warm"],
  ["play-checking", "stage", "cool"],
  ["play-starting", "stage", "cool"],
  ["client-setup", "stage", "cool"],
  ["client-repair", "stage", "cool"],
  ["play-maintenance", "stage", "plain"],
  ["play-offline", "stage", "plain"],
  ["play-key-changed", "stage", "plain"],
  ["play-unsupported", "stage", "plain"],
  ["welcome", "welcome", "cool"],
  ["sign-in", "signIn", "cool"],
] as const)("%s's ground is %s's %s", (file, screen, ground) => {
  expect(groundFor(golden(file), screen)).toBe(ground);
});

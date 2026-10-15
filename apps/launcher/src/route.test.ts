import { expect, test } from "vitest";

import { playVariant, route, type Tab } from "./route";
import { golden } from "./test-golden";

test.each([
  ["welcome", "play", "welcome"],
  ["redeem", "play", "redeem"],
  ["sign-in", "play", "signIn"],
  ["play-ready", "play", "play"],
  ["play-ready", "account", "account"],
  ["play-signed-out", "account", "signIn"],
  ["servers", "servers", "servers"],
  ["client-setup", "client", "clientSetup"],
  ["client-fetching", "client", "clientWorking"],
  ["client-downloading", "client", "clientWorking"],
  ["client-repair", "client", "clientRepair"],
  ["client-verified", "client", "clientReady"],
  ["settings", "settings", "settings"],
  ["welcome", "settings", "settings"],
])("%s on the %s tab shows %s", (file, tab, screen) => {
  expect(route(tab as Tab, golden(file))).toBe(screen);
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

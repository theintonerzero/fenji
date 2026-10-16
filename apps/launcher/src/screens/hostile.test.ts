// The launcher plan's Review Focus 2 on every screen, not Play alone (#348): what a server sends
// (its name, host, MOTD, version and maintenance message, the players in its lobby, and the words a
// notice carries) shows as text, and no element is made from it.
import { expect, test } from "vitest";

import type { ScreenFactory } from "../app";
import type { Snapshot } from "../types";
import { account } from "./account";
import { clientReady } from "./client-ready";
import { clientRepair } from "./client-repair";
import { clientSetup } from "./client-setup";
import { clientWorking } from "./client-working";
import { play } from "./play";
import { redeem } from "./redeem";
import { servers } from "./servers";
import { settings } from "./settings";
import { signIn } from "./sign-in";
import { golden, mount } from "./test-kit";
import { welcome } from "./welcome";

const HOSTILE = '<img src=x onerror="alert(1)">';

/** Every server-sent string in `s` replaced by markup. */
function hostile(s: Snapshot): Snapshot {
  for (const v of s.servers) {
    v.name = HOSTILE;
    v.host = HOSTILE;
    v.motd = HOSTILE;
    v.version = HOSTILE;
    if (v.username !== null) {
      v.username = HOSTILE;
    }
    if (v.status.kind === "maintenance") {
      v.status.message = HOSTILE;
    }
  }
  if (s.lobby !== null) {
    s.lobby = [HOSTILE, HOSTILE];
  }
  if (s.keyChange !== null) {
    s.keyChange.name = HOSTILE;
  }
  s.notice = { text: HOSTILE, error: true };
  return s;
}

const SCREENS: [golden: string, screen: ScreenFactory][] = [
  ["play-ready", play],
  ["play-checking", play],
  ["play-starting", play],
  ["play-running", play],
  ["play-maintenance", play],
  ["play-offline", play],
  ["play-key-changed", play],
  ["play-unsupported", play],
  ["play-ended", play],
  ["play-signed-out", signIn],
  ["sign-in", signIn],
  ["redeem", redeem],
  ["welcome", welcome],
  ["servers", servers],
  ["servers-key-change", servers],
  ["account", account],
  ["client-setup", clientSetup],
  ["client-verified", clientReady],
  ["client-repair", clientRepair],
  ["client-fetching", clientWorking],
  ["client-downloading", clientWorking],
  ["settings", settings],
];

test.each(SCREENS)("%s shows a hostile server's words as text and nothing more", (name, screen) => {
  mount(screen, hostile(golden(name)));
  const page = document.body;
  expect(page.querySelector("img")).toBeNull();
  expect(page.querySelector("[onerror]")).toBeNull();
  expect(page.textContent).toContain(HOSTILE);
});

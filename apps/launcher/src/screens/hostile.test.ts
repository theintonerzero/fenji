// The launcher plan's Review Focus 2 over every screen, popover and dialog (the redesign's Task 7):
// what a server sends (its name, host, notice, version and maintenance message, the players in its
// lobby, and the words a notice carries) shows as text, and no element is made from it.
import { expect, test } from "vitest";

import type { Ctx, PopoverContent } from "../app";
import { mount as mountApp } from "../app";
import { accountPopover } from "../popovers/account";
import { gamePopover } from "../popovers/game";
import { settingsPopover } from "../popovers/settings";
import type { Snapshot } from "../types";
import { keyDialog } from "../ui/key-dialog";
import { rail } from "../ui/rail";
import { removeDialog } from "../ui/server-dialogs";
import { redeem } from "./redeem";
import { signIn } from "./sign-in";
import { stage } from "./stage";
import { golden, mount, mountPopover, recording } from "./test-kit";
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

function clean(): void {
  expect(document.body.querySelector("img")).toBeNull();
  expect(document.body.querySelector("[onerror]")).toBeNull();
  expect(document.body.textContent).toContain(HOSTILE);
}

const STAGES = [
  "play-ready",
  "play-checking",
  "play-starting",
  "play-running",
  "play-maintenance",
  "play-offline",
  "play-key-changed",
  "play-unsupported",
  "play-ended",
  "client-setup",
  "client-repair",
];

test.each(STAGES)("the stage on %s shows a hostile server's words as text", (name) => {
  mount(stage, hostile(golden(name)));
  clean();
});

test.each([
  ["welcome", welcome],
  ["redeem", redeem],
  ["sign-in", signIn],
] as const)("%s shows a hostile server's words as text", (name, screen) => {
  mount(screen, hostile(golden(name)));
  // Welcome has no server to name: the notice carries the markup.
  clean();
});

test.each([
  ["game", gamePopover, "client-repair"],
  ["account", accountPopover, "servers"],
  ["settings", settingsPopover, "settings"],
] as [string, (ctx: Ctx) => PopoverContent, string][])("the %s popover shows hostile words as text", (which, make, name) => {
  const s = hostile(golden(name));
  // The Game popover shows no server's words, but a folder's name is the player's own, and anything.
  if (s.client.check.kind === "needsRepair") {
    s.client.check.files[0]!.path = HOSTILE;
  }
  const { root } = mountPopover(make, s);
  expect(root.querySelector("img")).toBeNull();
  if (which !== "settings") {
    expect(root.textContent).toContain(HOSTILE);
  }
});

test("the rail, a changed key's dialog and Remove show a hostile name as text", () => {
  const s = hostile(golden("servers-key-change"));
  const { ctx } = recording();
  const part = rail(ctx);
  part.update(s, false);
  const dialog = keyDialog(ctx, s);
  const remove = removeDialog(ctx, s.servers[0]!, () => {});
  document.body.replaceChildren(part.el, dialog, remove.el);
  part.el.querySelector<HTMLButtonElement>("button[data-server]")!.focus();
  clean();
});

test("the whole frame, top bar and toast included, on a hostile snapshot", async () => {
  const s = hostile(golden("update-ready"));
  const root = document.createElement("div");
  document.body.replaceChildren(root);
  await mountApp(root, { snapshot: async () => s, onSnapshot: async () => () => {}, send: async () => null });
  clean();
});

import { expect, test } from "vitest";

import type { Ctx } from "../app";
import type { Intent, Snapshot } from "../types";
import { servers } from "./servers";
import { embers, golden, mount, press, settle } from "./test-kit";

test("three servers, one ember Play on the selected row, and every state in words", () => {
  const { root, sent } = mount(servers, golden("servers"), undefined, "servers");
  const rows = root.querySelectorAll("[data-server]");
  expect(rows.length).toBe(3);
  expect(embers(root)).toBe(1);
  expect(rows[1]!.textContent).toContain("In maintenance");
  // In maintenance, the host's message is the row's notice line (the Servers board).
  expect(rows[1]!.textContent).toContain("Mei: moving the server to a new machine. Back around 21:00.");
  expect(rows[2]!.textContent).toContain("Can't reach");
  // A row keeps the faint dot, as the Servers board draws it; Play and the column carry the icon.
  expect(rows[2]!.querySelector(".server-status .dot")).not.toBeNull();
  expect(rows[2]!.querySelector(".status-icon")).toBeNull();
  (rows[1] as HTMLElement).click();
  expect(sent).toContainEqual({ type: "selectServer", id: golden("servers").servers[1]!.id });
});

test("the Add dialog sends what was typed and shows a refusal under the field", async () => {
  const sent: Intent[] = [];
  const root = document.createElement("main");
  document.body.replaceChildren(root);
  const screen = servers(root, {
    tab: "servers",
    setTab() {},
    send: async (i: Intent) => {
      sent.push(i);
      return "That isn't a server address or an invite link. Paste the link your host sent.";
    },
  } as Ctx);
  screen.update(golden("servers"));
  (root.querySelector('[data-action="add"]') as HTMLElement).click();
  const dialog = document.querySelector('[role="dialog"]')!;
  (dialog.querySelector("input") as HTMLInputElement).value = "nonsense";
  (dialog.querySelector("form") as HTMLFormElement).requestSubmit();
  await new Promise((r) => setTimeout(r));
  expect(sent).toEqual([{ type: "addServer", text: "nonsense" }]);
  expect(dialog.textContent).toContain("That isn't a server address or an invite link. Paste the link your host sent.");
  expect(dialog.querySelector('[aria-invalid="true"]')).not.toBeNull();
  press(dialog as HTMLElement, "Cancel");
  expect(document.querySelector('[role="dialog"]')).toBeNull();
  // The refusal waited as the notice too: closing the dialog dismisses it.
  expect(sent.at(-1)).toEqual({ type: "dismissNotice" });
});

test("a server the model took closes the Add dialog", async () => {
  const { root, sent } = mount(servers, golden("servers"), undefined, "servers");
  press(root, "Add server");
  const dialog = document.querySelector('[role="dialog"]')!;
  (dialog.querySelector("input") as HTMLInputElement).value = "jadehall.example.org";
  (dialog.querySelector("form") as HTMLFormElement).requestSubmit();
  await settle();
  expect(sent).toEqual([{ type: "addServer", text: "jadehall.example.org" }]);
  expect(document.querySelector('[role="dialog"]')).toBeNull();
});

test("Play on the selected row plays there, and Refresh probes", () => {
  const { root, sent } = mount(servers, golden("servers"), undefined, "servers");
  (root.querySelector("[data-server] .ember") as HTMLButtonElement).click();
  press(root, "Refresh");
  expect(sent).toEqual([{ type: "playOn", id: 1 }, { type: "refresh" }]);
});

test("the row's PLAY waits for a verified copy and no check running, as Play's does (#348)", () => {
  for (const change of [
    (s: Snapshot) => (s.client.check = { kind: "none" }),
    (s: Snapshot) => (s.job = { kind: "checking", level: "full", filesDone: 3, filesTotal: 210, percent: 1 }),
  ]) {
    const s = golden("servers");
    change(s);
    const { root } = mount(servers, s, undefined, "servers");
    expect(root.querySelector("[data-server] .btn-play")).toBeNull();
  }
});

test("the column has the selected server's facts, its source, and Remove with a confirmation", () => {
  const { root, sent } = mount(servers, golden("servers"), undefined, "servers");
  const column = root.querySelector("aside")!;
  expect(column.textContent).toContain("24 ms");
  expect(column.textContent).toContain("Hosted by");
  press(column, "Source code");
  press(column, "Remove Moonward from the list");
  const confirm = document.querySelector('[role="dialog"]')!;
  expect(confirm.textContent).toContain("Remove Moonward from your list? Your account on it stays.");
  press(confirm as HTMLElement, "Remove");
  expect(sent).toEqual([
    { type: "openUrl", url: "https://github.com/theintonerzero/fenji" },
    { type: "removeServer", id: 1 },
  ]);
  expect(document.querySelector('[role="dialog"]')).toBeNull();
});

test("a server without a source has no Source code link, and a pinned one says own key", () => {
  const s = golden("servers");
  s.active = 3;
  const { root } = mount(servers, s, undefined, "servers");
  expect(root.querySelector("aside")!.textContent).not.toContain("Source code");
  expect(root.querySelectorAll("[data-server]")[2]!.textContent).toContain("own key");
  expect(embers(root)).toBe(0);
});

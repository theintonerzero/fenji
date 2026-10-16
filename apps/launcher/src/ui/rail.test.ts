import { expect, test } from "vitest";

import { golden, recording } from "../screens/test-kit";
import { rail } from "./rail";

function mounted(file = "servers") {
  const { ctx, asked } = recording();
  const part = rail(ctx);
  document.body.replaceChildren(part.el);
  part.update(golden(file), false);
  const seals = [...part.el.querySelectorAll<HTMLButtonElement>("button.seal[data-server]")];
  return { part, asked, seals };
}

const key = (el: Element, key: string, shiftKey = false): void =>
  void el.dispatchEvent(new KeyboardEvent("keydown", { key, shiftKey, bubbles: true }));

test("each server is a seal with its state in a bar, the active one marked, then +", () => {
  const { part, seals } = mounted();
  expect(seals.map((s) => s.textContent)).toEqual(["M", "J", "E"]);
  expect(seals.map((s) => s.dataset.bar)).toEqual(["ok", "warn", "off"]);
  expect(seals.map((s) => s.getAttribute("aria-current"))).toEqual(["true", null, null]);
  expect(seals.map((s) => s.getAttribute("aria-label"))).toEqual([
    "Moonward, 3/10 players",
    "Jade Hall, Maintenance",
    "Ember Peak, Offline",
  ]);
  expect(part.el.querySelector('button[aria-label="Add a server"]')).not.toBeNull();
  expect(part.el.hasAttribute("data-tauri-drag-region")).toBe(true);
});

test("a server the launcher can't play on has a cinnabar bar", () => {
  const { seals } = mounted("play-key-changed");
  expect(seals[0]!.dataset.bar).toBe("bad");
});

test("a click chooses a server, + adds one, and + shows it's open while its dialog is", () => {
  const { part, asked, seals } = mounted();
  seals[1]!.click();
  const add = part.el.querySelector<HTMLButtonElement>('button[aria-label="Add a server"]')!;
  add.click();
  expect(asked.selected).toEqual([2]);
  expect(asked.adds).toBe(1);
  part.update(golden("servers"), true);
  expect(add.classList.contains("open")).toBe(true);
});

test("the arrow keys move along the rail, + included, and a seal's tip shows its name and players", () => {
  const { part, seals } = mounted();
  seals[0]!.focus();
  const tip = part.el.querySelector(".tip")!;
  expect(tip.hasAttribute("hidden")).toBe(false);
  expect(tip.textContent).toBe("Moonward3/10 players");
  key(seals[0]!, "ArrowDown");
  expect(document.activeElement).toBe(seals[1]);
  key(seals[1]!, "End");
  expect(document.activeElement?.getAttribute("aria-label")).toBe("Add a server");
  key(document.activeElement!, "Home");
  expect(document.activeElement).toBe(seals[0]);
});

test("a seal's menu has its source code and Remove, from the keyboard too; Delete asks to remove", () => {
  const { part, asked, seals } = mounted();
  key(seals[0]!, "F10", true);
  const menu = part.el.querySelector('[role="menu"]')!;
  expect([...menu.querySelectorAll('[role="menuitem"]')].map((b) => b.textContent)).toEqual(["Source code", "Remove from list"]);
  expect(document.activeElement?.textContent).toBe("Source code");
  key(document.activeElement!, "ArrowDown");
  (document.activeElement as HTMLButtonElement).click();
  expect(asked.removes.map((v) => v.id)).toEqual([1]);
  expect(part.el.querySelector('[role="menu"]')).toBeNull();
  expect(document.activeElement).toBe(seals[0]);
  // A server without a source link has Remove alone.
  seals[2]!.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true }));
  expect([...part.el.querySelectorAll('[role="menuitem"]')].map((b) => b.textContent)).toEqual(["Remove from list"]);
  key(part.el.querySelector('[role="menuitem"]')!, "Escape");
  key(seals[1]!, "Delete");
  expect(asked.removes.map((v) => v.id)).toEqual([1, 2]);
});

test("a probe that draws the seals again keeps focus on the same seal", () => {
  const { part, seals } = mounted();
  seals[2]!.focus();
  const s = golden("servers");
  s.servers[2]!.status = { kind: "online" };
  part.update(s, false);
  expect(document.activeElement?.getAttribute("data-server")).toBe("3");
});

test("a seal's menu stays open while a probe redraws the rail, which catches up when it closes", () => {
  const { part, seals } = mounted();
  key(seals[2]!, "F10", true);
  const s = golden("servers");
  s.servers[2]!.status = { kind: "online" };
  part.update(s, false);
  expect(part.el.querySelector('[role="menu"]')).not.toBeNull();
  expect(document.activeElement?.getAttribute("role")).toBe("menuitem");
  key(document.activeElement!, "Escape");
  expect(part.el.querySelector('[role="menu"]')).toBeNull();
  expect(part.el.querySelector<HTMLElement>('button[data-server="3"]')!.dataset.bar).toBe("ok");
});

test("when the focused seal's server is removed, focus goes to the next seal, then the last, then + (#376)", () => {
  const { part } = mounted();
  const sealFor = (id: number) => part.el.querySelector<HTMLButtonElement>(`button[data-server="${id}"]`)!;
  const without = (ids: number[]) => {
    const s = golden("servers");
    s.servers = s.servers.filter((v) => !ids.includes(v.id));
    s.active = s.servers[0]?.id ?? null;
    return s;
  };
  sealFor(1).focus();
  part.update(without([1]), false);
  expect(document.activeElement).toBe(sealFor(2));
  // The last seal's server: the one before it.
  sealFor(3).focus();
  part.update(without([1, 3]), false);
  expect(document.activeElement).toBe(sealFor(2));
  // The only one: +.
  part.update(without([1, 2, 3]), false);
  expect(document.activeElement).toBe(part.el.querySelector('button[aria-label="Add a server"]'));
});

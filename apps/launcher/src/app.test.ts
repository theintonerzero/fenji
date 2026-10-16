import { expect, test } from "vitest";

import { mount } from "./app";
import { settle } from "./screens/test-kit";
import { golden } from "./test-golden";
import type { Intent, Snapshot } from "./types";

async function mounted(first: Snapshot | (() => Snapshot)) {
  const root = document.createElement("div");
  document.body.replaceChildren(root);
  const sent: Intent[] = [];
  let push: (s: Snapshot) => void = () => {};
  await mount(root, {
    snapshot: async () => (typeof first === "function" ? first() : first),
    onSnapshot: async (cb) => {
      push = cb;
      return () => {};
    },
    send: async (intent) => {
      sent.push(intent);
      return null;
    },
  });
  return { root, sent, push: (s: Snapshot) => push(s) };
}

const control = (root: HTMLElement, label: string) => root.querySelector<HTMLButtonElement>(`.top button[aria-label^="${label}"]`)!;

test("the rail and the stage: the active server's name, and no tabs or column; the view never sends a tab", async () => {
  const { root, sent } = await mounted(golden("servers"));
  expect(root.querySelector(".rail")).not.toBeNull();
  expect(root.querySelector(".stage .name")!.textContent).toBe("Moonward");
  expect(root.querySelector(".stage")!.getAttribute("data-ground")).toBe("warm");
  expect(root.querySelector('[role="tab"], .tabs, .column')).toBeNull();
  expect(sent.filter((i) => i.type === "viewTab")).toEqual([]);
});

test("a popover opens under its control, takes focus, and Escape gives it back", async () => {
  const { root } = await mounted(golden("play-ready"));
  const chip = control(root, "Game");
  chip.click();
  await settle();
  const panel = root.querySelector<HTMLElement>('.popover[role="dialog"]')!;
  expect(panel.querySelector(".popover-title")!.textContent).toBe("Game");
  expect(chip.getAttribute("aria-expanded")).toBe("true");
  expect(panel.contains(document.activeElement)).toBe(true);
  panel.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
  expect(root.querySelector(".popover")).toBeNull();
  expect(chip.getAttribute("aria-expanded")).toBe("false");
  expect(document.activeElement).toBe(chip);
  // Another control switches; the same one closes; the scrim closes.
  control(root, "Settings").click();
  expect(root.querySelector(".popover-title")!.textContent).toBe("Settings");
  control(root, "Account").click();
  expect(root.querySelectorAll(".popover")).toHaveLength(1);
  expect(root.querySelector(".popover-title")!.textContent).toBe("Account");
  control(root, "Account").click();
  expect(root.querySelector(".popover")).toBeNull();
  control(root, "Settings").click();
  root.querySelector(".popover-scrim")!.dispatchEvent(new MouseEvent("mousedown", { bubbles: true }));
  expect(root.querySelector(".popover")).toBeNull();
});

test("a changed key's dialog comes over the stage when the model asks, and goes when it's answered", async () => {
  const { root, push } = await mounted(golden("servers-key-change"));
  expect(root.querySelector('.dialog[role="dialog"]')!.textContent).toContain("A new key for Ember Peak");
  push(golden("servers"));
  expect(root.querySelector('.dialog[role="dialog"]')).toBeNull();
});

test("an update waits as a toast, not a dialog", async () => {
  const { root, sent } = await mounted(golden("update-ready"));
  expect(root.querySelector('.dialog[role="dialog"]')).toBeNull();
  const toast = root.querySelector('.toast[role="status"]')!;
  expect(toast.textContent).toContain("0.5.1 · installs on restart");
  toast.querySelector<HTMLButtonElement>('button[aria-label="Later"]')!.click();
  expect(sent.at(-1)).toEqual({ type: "dismissUpdate" });
});

test("Sign in from the Account popover goes to that server, then its sign-in", async () => {
  const { root, sent, push } = await mounted(golden("servers"));
  control(root, "Account").click();
  [...root.querySelectorAll<HTMLButtonElement>(".popover button")].find((b) => b.textContent === "Sign in")!.click();
  await settle();
  expect(sent.at(-1)).toEqual({ type: "selectServer", id: 2 });
  expect(root.querySelector(".popover")).toBeNull();
  const there = golden("servers");
  there.active = 2;
  push(there);
  expect(root.querySelector(".stage-body h1")!.textContent).toBe("Sign in to Jade Hall");
  // A seal chosen in the rail goes to its stage, not the sign-in.
  root.querySelector<HTMLButtonElement>('.rail button[data-server="1"]')!.click();
  push(golden("servers"));
  expect(root.querySelector(".stage-body .name")!.textContent).toBe("Moonward");
});

test("+ opens Add a server over the stage, and + shows it open", async () => {
  const { root } = await mounted(golden("play-ready"));
  const add = root.querySelector<HTMLButtonElement>('.rail button[aria-label="Add a server"]')!;
  add.click();
  expect(root.querySelector(".dialog-title")!.textContent).toBe("Add a server");
  expect(add.classList.contains("open")).toBe(true);
  [...root.querySelectorAll<HTMLButtonElement>(".dialog button")].find((b) => b.textContent === "Cancel")!.click();
  expect(root.querySelector(".dialog")).toBeNull();
  expect(add.classList.contains("open")).toBe(false);
});

test("a change while the view fetches its first snapshot isn't lost: it listens first (#348)", async () => {
  let fetches = 0;
  let pushed: ((s: Snapshot) => void) | null = null;
  const root = document.createElement("div");
  document.body.replaceChildren(root);
  await mount(root, {
    snapshot: async () => {
      fetches += 1;
      if (fetches === 1) {
        // Maintenance begins while the view asks: the model publishes it, then the answer comes.
        pushed!(golden("play-maintenance"));
        return golden("play-ready");
      }
      return golden("play-maintenance");
    },
    onSnapshot: async (cb) => {
      pushed = cb;
      return () => {};
    },
    send: async () => null,
  });
  expect(root.querySelector(".state")!.textContent).toMatch(/^Maintenance/);
});

/** The servers golden with Jade Hall (2) active, and Play's sign-in waiting there. */
function waitingOnJadeHall(): Snapshot {
  const s = golden("servers");
  s.active = 2;
  s.needSignIn = true;
  return s;
}

const title = (root: HTMLElement) => root.querySelector(".stage-body h1")!.textContent;

test("Play's sign-in stays on its server: another seal leads to its stage, and the sign-in is cancelled", async () => {
  const { root, sent, push } = await mounted(waitingOnJadeHall());
  expect(title(root)).toBe("Sign in to Jade Hall");
  root.querySelector<HTMLInputElement>('input[type="password"]')!.value = "a long password";
  root.querySelector<HTMLButtonElement>('.rail button[data-server="1"]')!.click();
  // The model still waits for the sign-in when the choice of Moonward comes back.
  const moonward = waitingOnJadeHall();
  moonward.active = 1;
  push(moonward);
  expect(title(root)).toBe("Moonward");
  expect(root.querySelector('input[type="password"]')).toBeNull();
  push(moonward);
  expect(sent.filter((i) => i.type === "cancelSignIn")).toHaveLength(1);
});

test("an invite added while a sign-in shows opens Join for its own server", async () => {
  const { root, push } = await mounted(golden("servers"));
  control(root, "Account").click();
  [...root.querySelectorAll<HTMLButtonElement>(".popover button")].find((b) => b.textContent === "Sign in")!.click();
  const jade = golden("servers");
  jade.active = 2;
  push(jade);
  expect(title(root)).toBe("Sign in to Jade Hall");
  const invited = golden("servers");
  invited.servers.push({ ...invited.servers[2]!, id: 4, name: "Stone Gate" });
  invited.active = 4;
  invited.pendingInvite = { serverId: 4, code: "K7QM-3XRD-VN2P-HT8B" };
  push(invited);
  expect(title(root)).toBe("Join Stone Gate");
});

test("cancelling Remove keeps the sign-in being typed", async () => {
  const { root, push } = await mounted(golden("servers"));
  control(root, "Account").click();
  [...root.querySelectorAll<HTMLButtonElement>(".popover button")].find((b) => b.textContent === "Sign in")!.click();
  const jade = golden("servers");
  jade.active = 2;
  push(jade);
  root.querySelector<HTMLButtonElement>('.rail button[data-server="3"]')!.dispatchEvent(
    new KeyboardEvent("keydown", { key: "Delete", bubbles: true }),
  );
  [...root.querySelectorAll<HTMLButtonElement>(".dialog button")].find((b) => b.textContent === "Cancel")!.click();
  expect(title(root)).toBe("Sign in to Jade Hall");
});

test.each([true, false])("Welcome's address goes on to sign in there, whichever comes first (snapshot first: %s)", async (snapshotFirst) => {
  const root = document.createElement("div");
  document.body.replaceChildren(root);
  let push: (s: Snapshot) => void = () => {};
  await mount(root, {
    snapshot: async () => golden("welcome"),
    onSnapshot: async (cb) => {
      push = cb;
      return () => {};
    },
    send: async (intent) => {
      if (intent.type === "addServer" && snapshotFirst) {
        push(golden("play-signed-out"));
      }
      return null;
    },
  });
  [...root.querySelectorAll<HTMLButtonElement>("button")].find((b) => b.textContent === "I have an account")!.click();
  root.querySelector<HTMLInputElement>("input")!.value = "moonward.example.net";
  root.querySelector<HTMLFormElement>("form")!.requestSubmit();
  await settle();
  if (!snapshotFirst) {
    push(golden("play-signed-out"));
  }
  expect(title(root)).toBe("Sign in to Moonward");
});

test("Shift+Tab from an opened popover stays in it, and Escape closes it wherever focus fell (R7)", async () => {
  const { root } = await mounted(golden("play-ready"));
  control(root, "Settings").click();
  await settle();
  const panel = root.querySelector<HTMLElement>(".popover")!;
  expect(document.activeElement).toBe(panel);
  panel.dispatchEvent(new KeyboardEvent("keydown", { key: "Tab", shiftKey: true, bubbles: true, cancelable: true }));
  expect(panel.contains(document.activeElement)).toBe(true);
  expect(document.activeElement).not.toBe(panel);
  // Focus fell out of it, as when a control inside is redrawn: Escape still closes it.
  (document.activeElement as HTMLElement).blur();
  document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
  expect(root.querySelector(".popover")).toBeNull();
});

test("a changed key arriving while a popover is open closes it and asks once", async () => {
  const { root, push } = await mounted(golden("servers"));
  control(root, "Game").click();
  push(golden("servers-key-change"));
  expect(root.querySelector(".popover")).toBeNull();
  expect(root.querySelectorAll('.dialog[role="dialog"]')).toHaveLength(1);
  push(golden("servers"));
  expect(root.querySelectorAll('.dialog[role="dialog"]')).toHaveLength(0);
});

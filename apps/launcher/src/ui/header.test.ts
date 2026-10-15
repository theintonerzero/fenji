import { expect, test } from "vitest";

import type { Ctx } from "../app";
import { golden } from "../test-golden";
import type { Intent } from "../types";
import { header } from "./header";

test("the header has four tabs, the active one marked, and no maximise", () => {
  const sent: Intent[] = [];
  const ctx: Ctx = {
    tab: "servers",
    setTab() {},
    send: async (intent) => {
      sent.push(intent);
      return null;
    },
  };
  const el = header(ctx, golden("play-ready"));
  const tabs = [...el.querySelectorAll('[role="tab"], nav a, nav button')].map((t) => t.textContent);
  expect(tabs).toEqual(["Play", "Servers", "Account", "Client"]);
  expect(el.querySelector('[aria-current="page"]')!.textContent).toBe("Servers");
  expect(el.querySelector('[aria-label="Maximise"]')).toBeNull();
  (el.querySelector('[aria-label="Minimise"]') as HTMLElement).click();
  expect(sent).toEqual([{ type: "minimize" }]);
});

test("signed out, the pill offers Sign in, except on the screens that are the sign-in", () => {
  const ctx: Ctx = { tab: "play", setTab() {}, send: async () => null };
  const pill = (name: string, tab: Ctx["tab"]) => {
    ctx.tab = tab;
    return header(ctx, golden(name)).querySelector<HTMLButtonElement>(".pill")!;
  };
  expect(pill("play-signed-out", "play").hidden).toBe(false);
  expect(pill("play-signed-out", "play").textContent).toBe("Sign in");
  expect(pill("play-signed-out", "account").hidden).toBe(true);
  expect(pill("welcome", "play").hidden).toBe(true);
  expect(pill("redeem", "play").hidden).toBe(true);
  expect(pill("play-ready", "account").hidden).toBe(false);
});

test("an account task marks Account, whichever tab routed to it, as the Welcome, Redeem and SignIn boards do", () => {
  // The finish review's fix 5: route() shows these screens over the tab that was clicked.
  for (const [state, tab] of [
    ["welcome", "play"],
    ["redeem", "client"],
    ["play-signed-out", "account"],
  ] as const) {
    const ctx: Ctx = { tab, setTab() {}, send: async () => null };
    const el = header(ctx, golden(state));
    expect(el.querySelector('[aria-current="page"]')!.textContent, `${state} on ${tab}`).toBe("Account");
  }
  // A screen of the clicked tab's own keeps its tab.
  const ctx: Ctx = { tab: "client", setTab() {}, send: async () => null };
  expect(header(ctx, golden("play-ready")).querySelector('[aria-current="page"]')!.textContent).toBe("Client");
});

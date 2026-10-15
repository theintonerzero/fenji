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

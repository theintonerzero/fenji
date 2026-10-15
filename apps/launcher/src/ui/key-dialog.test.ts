import { expect, test } from "vitest";

import type { Ctx } from "../app";
import { golden } from "../screens/test-kit";
import type { Intent } from "../types";
import { keyDialog } from "./key-dialog";

function open(hadKey: boolean) {
  const s = golden("servers-key-change");
  s.keyChange!.hadKey = hadKey;
  const sent: Intent[] = [];
  const el = keyDialog({ send: async (i: Intent) => (sent.push(i), null) } as Ctx, s);
  document.body.replaceChildren(el);
  return { el, sent };
}

test("a different key asks first: Use the new key is the one ember action", () => {
  const { el, sent } = open(true);
  expect(el.textContent).toContain(
    "This link carries a different key for Ember Peak than the one saved. Use it only if your host told you the server's key changed.",
  );
  expect(el.querySelectorAll(".ember")).toHaveLength(1);
  (el.querySelector(".ember") as HTMLElement).click();
  [...el.querySelectorAll("button")].find((b) => b.textContent === "Keep the saved key")!.click();
  expect(sent).toEqual([{ type: "useNewKey", id: 3 }, { type: "keepKey" }]);
});

test("a first key for a server saved without one says so", () => {
  const { el } = open(false);
  expect(el.textContent).toContain("This link carries a key for Ember Peak, which was saved without one.");
});

import { expect, test } from "vitest";

import { golden, recording } from "../screens/test-kit";
import { keyDialog } from "./key-dialog";

function open(hadKey: boolean) {
  const s = golden("servers-key-change");
  s.keyChange!.hadKey = hadKey;
  const { ctx, asked } = recording();
  const el = keyDialog(ctx, s);
  document.body.replaceChildren(el);
  return { el, sent: asked.sent };
}

test("a different key asks first: Use the new key is the one ember action", () => {
  const { el, sent } = open(true);
  expect(el.querySelector(".dialog-title")!.textContent).toBe("A new key for Ember Peak");
  expect(el.textContent).toContain("This link carries a different key. Use it only if your host told you the key changed.");
  expect(el.querySelectorAll(".ember")).toHaveLength(1);
  (el.querySelector(".ember") as HTMLElement).click();
  [...el.querySelectorAll("button")].find((b) => b.textContent === "Keep the old key")!.click();
  expect(sent).toEqual([{ type: "useNewKey", id: 3 }, { type: "keepKey" }]);
});

test("a first key for a server saved without one says so", () => {
  const { el } = open(false);
  expect(el.querySelector(".dialog-title")!.textContent).toBe("A key for Ember Peak");
  expect(el.textContent).toContain("Use it only if it came from your host.");
  expect([...el.querySelectorAll("button")].map((b) => b.textContent)).toEqual(["Not now", "Use this key"]);
});

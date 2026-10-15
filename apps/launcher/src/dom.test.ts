import { expect, test } from "vitest";

import { h } from "./dom";

test("text is only ever text", () => {
  const hostile = "<img src=x onerror=alert(1)>";
  const el = h("p", {}, hostile, h("span", { title: hostile }, hostile));
  document.body.replaceChildren(el);
  expect(el.textContent).toBe(hostile + hostile);
  expect(document.querySelector("img")).toBeNull();
});

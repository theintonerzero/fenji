import { expect, test, vi } from "vitest";

import { h, s } from "./dom";

test("text is only ever text", () => {
  const hostile = "<img src=x onerror=alert(1)>";
  const el = h("p", {}, hostile, h("span", { title: hostile }, hostile));
  document.body.replaceChildren(el);
  expect(el.textContent).toBe(hostile + hostile);
  expect(document.querySelector("img")).toBeNull();
});

test("a style goes through CSSOM, which the shell's CSP allows, never through setAttribute (#325)", () => {
  const set = vi.spyOn(Element.prototype, "setAttribute");
  try {
    const el = h("div", { style: "top: 3px", class: "readout" });
    s("stop", { offset: 0, style: "stop-color: var(--ember-500)" });
    expect(set.mock.calls.filter(([name]) => name === "style")).toEqual([]);
    expect(el.style.top).toBe("3px");
    expect(el.getAttribute("class")).toBe("readout");
  } finally {
    set.mockRestore();
  }
});

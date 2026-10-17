import { readFileSync } from "node:fs";

import { expect, test } from "vitest";

import { rule } from "./rule";

test("the view fills the window it's given, whatever its size in CSS pixels (#383)", () => {
  // 1200 × 720 is the window's design size, not the page's: where the desktop's scale gives the
  // webview a larger page, a fixed size left a band at the window's bottom and right.
  const base = readFileSync(`${process.cwd()}/src/styles/base.css`, "utf8");
  const components = readFileSync(`${process.cwd()}/src/styles/components.css`, "utf8");
  for (const [css, selector] of [
    [base, "body"],
    [components, ".window"],
  ] as const) {
    const declarations = rule(css, selector);
    expect(declarations, selector).toMatch(/\bwidth: 100%;/);
    expect(declarations, selector).toMatch(/\bheight: 100%;/);
    expect(declarations, selector).not.toMatch(/\b(width|height): \d+px;/);
  }
});

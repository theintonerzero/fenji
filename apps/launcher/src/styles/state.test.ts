import { readFileSync } from "node:fs";

import { expect, test } from "vitest";

import { rule } from "./rule";

test("the line of state keeps its spaces, and a long region gives way first (hosting spec Amendment C, HC5)", () => {
  // The view's tests read text, not layout: a flex item drops the space at its start, so without
  // pre " · Germany" draws as "· Germany" though its text is right (#398's review).
  const css = readFileSync(`${process.cwd()}/src/styles/components.css`, "utf8");
  expect(rule(css, ".ping")).toMatch(/\bwhite-space: pre;/);
  const region = rule(css, ".region");
  expect(region).toMatch(/\bwhite-space: pre;/);
  expect(region).toMatch(/\btext-overflow: ellipsis;/);
  expect(region).toMatch(/\boverflow: hidden;/);
  expect(region).toMatch(/\bmin-width: 0;/);
  expect(rule(css, ".state-live")).toMatch(/\bflex: none;/);
});

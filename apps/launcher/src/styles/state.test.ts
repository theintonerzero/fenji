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

test("the server's version follows its address in the top bar, never cut short (the boards, R1)", () => {
  // The address gives way, with an ellipsis; the version keeps its " · ", as a flex item drops the
  // space at its start without pre.
  const css = readFileSync(`${process.cwd()}/src/styles/components.css`, "utf8");
  expect(rule(css, ".address")).toMatch(/\btext-overflow: ellipsis;/);
  const version = rule(css, ".server-version");
  expect(version).toMatch(/\bwhite-space: pre;/);
  expect(version).toMatch(/\bflex: none;/);
  expect(version).toMatch(/\bcolor: var\(--faint\);/);
  // One item with the address, so the bar's gap between its items never falls between them.
  expect(rule(css, ".where")).toMatch(/\bdisplay: flex;/);
  // The controls never shrink for them.
  expect(rule(css, ".top > :is(.chip, .me, .icon-button, .window-controls)")).toMatch(/\bflex: none;/);
});

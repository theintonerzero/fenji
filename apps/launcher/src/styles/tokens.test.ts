import { readFileSync } from "node:fs";

import { expect, test } from "vitest";

test("every colour in DESIGN.md's front matter is a token, with its value", () => {
  // The tests run from apps/launcher.
  const design = readFileSync(`${process.cwd()}/../../DESIGN.md`, "utf8");
  const tokens = readFileSync(`${process.cwd()}/src/styles/tokens.css`, "utf8");
  const front = design.split("---")[1]!;
  const colours = front.slice(front.indexOf("colors:"), front.indexOf("typography:"));
  const pairs = [...colours.matchAll(/^ {2}([\w-]+): "([^"]+)"$/gm)].map((m) => [m[1], m[2]]);
  expect(pairs.length).toBeGreaterThan(30);
  for (const [name, value] of pairs) {
    expect(tokens).toContain(`--${name}: ${value};`);
  }
});

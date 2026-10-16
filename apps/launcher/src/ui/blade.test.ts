import { expect, test } from "vitest";

import { blade } from "./blade";

test("the climbing blade is a progress bar with words; the others are art", () => {
  const climbing = blade({ kind: "climbing", percent: 78, label: "164 of 210 files" });
  expect(climbing.getAttribute("role")).toBe("progressbar");
  expect(climbing.getAttribute("aria-valuenow")).toBe("78");
  expect(climbing.getAttribute("aria-valuetext")).toBe("78%, 164 of 210 files");
  for (const kind of ["unlit", "lit", "bloom"] as const) {
    const art = blade({ kind });
    expect(art.getAttribute("role")).toBeNull();
    expect(art.getAttribute("aria-hidden")).toBe("true");
  }
});

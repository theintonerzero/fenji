import { expect, test } from "vitest";

import { golden } from "./test-golden";
import { statusLine } from "./words";

test("status lines say what is true", () => {
  const now = Date.UTC(2026, 9, 15, 20, 14);
  expect(statusLine(golden("play-ready"), now)).toBe("Online · 3 friends playing · hosted by Wen");
  expect(statusLine(golden("play-running"), now)).toBe(
    "Online · you and 3 friends playing · hosted by Wen",
  );
  expect(statusLine(golden("play-signed-out"), now)).toBe("Online · 3 of 10 playing · hosted by Wen");
  expect(statusLine(golden("play-maintenance"), now)).toMatch(
    /^In maintenance · back around \d\d:\d\d · hosted by Wen$/,
  );
  expect(statusLine(golden("play-offline"), now)).toMatch(
    /^Can't reach Moonward · last seen (today|yesterday) at \d\d:\d\d$|^Can't reach Moonward · last seen on \d+ \w{3}$/,
  );
});

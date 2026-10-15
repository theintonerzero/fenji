import { expect, test } from "vitest";

import type { Ctx } from "../app";
import { golden } from "../screens/test-kit";
import type { Intent } from "../types";
import { updateDialog } from "./update-dialog";

test("one ember button, and the notes as text", () => {
  const s = golden("update-ready");
  s.update.notes = "<b>bold</b>";
  const sent: Intent[] = [];
  const el = updateDialog(
    {
      send: async (i: Intent) => {
        sent.push(i);
        return null;
      },
    } as Ctx,
    s,
  );
  document.body.replaceChildren(el);
  expect(el.querySelectorAll(".ember").length).toBe(1);
  expect(el.querySelector("b")).toBeNull();
  (el.querySelector(".ember") as HTMLElement).click();
  expect(sent).toEqual([{ type: "installUpdate" }]);
});

test("it names both versions, lists the notes, and Later dismisses it", () => {
  const sent: Intent[] = [];
  const el = updateDialog({ send: async (i: Intent) => (sent.push(i), null) } as Ctx, golden("update-ready"));
  document.body.replaceChildren(el);
  expect(el.textContent).toContain("Fenji launcher 0.5.1 is ready");
  expect(el.textContent).toContain("You have 0.5.0; restarting installs 0.5.1 and opens the launcher again.");
  expect(el.querySelectorAll(".bullets li")).toHaveLength(3);
  expect(el.textContent).toContain("Signed with Fenji's release key.");
  [...el.querySelectorAll("button")].find((b) => b.textContent === "Later")!.click();
  expect(sent).toEqual([{ type: "dismissUpdate" }]);
});

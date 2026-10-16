import { expect, test } from "vitest";

import { golden, recording } from "../screens/test-kit";
import { updateToast } from "./toast";

test("an update waits in a corner: Restart installs it, Later puts it away", () => {
  const { ctx, asked } = recording();
  const el = updateToast(ctx, golden("update-ready"));
  document.body.replaceChildren(el);
  expect(el.getAttribute("role")).toBe("status");
  expect(el.textContent).toContain("Update ready");
  expect(el.textContent).toContain("0.5.1 · installs on restart");
  [...el.querySelectorAll("button")].find((b) => b.textContent === "Restart")!.click();
  el.querySelector<HTMLButtonElement>('button[aria-label="Later"]')!.click();
  expect(asked.sent).toEqual([{ type: "installUpdate" }, { type: "dismissUpdate" }]);
});

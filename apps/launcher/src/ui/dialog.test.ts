import { expect, test } from "vitest";

import { dialog } from "./dialog";

test("a title, its description, the body, then the actions; Escape closes it", () => {
  let closed = 0;
  const first = document.createElement("button");
  first.textContent = "Cancel";
  const d = dialog({ title: "Add a server", description: "Paste the link.", actions: [first], onClose: () => (closed += 1) });
  document.body.replaceChildren(d.el);
  const box = d.el.querySelector('[role="dialog"]')!;
  expect(box.getAttribute("aria-modal")).toBe("true");
  expect(document.getElementById(box.getAttribute("aria-labelledby")!)!.textContent).toBe("Add a server");
  expect(box.querySelector(".dialog-description")!.textContent).toBe("Paste the link.");
  box.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
  expect(closed).toBe(1);
});

import { expect, test } from "vitest";

import { dialog } from "./dialog";

test("the description sits under the title, beside the tile, as the AddServer and Update boards draw it", () => {
  // The finish review's fix 7: it ran full width beneath the tile.
  const d = dialog({
    title: "Add a server",
    icon: document.createElement("span"),
    description: "Paste the link.",
    actions: [],
    onClose() {},
  });
  const heading = d.el.querySelector(".dialog-head > .dialog-heading")!;
  expect(heading.querySelector(".dialog-title")!.textContent).toBe("Add a server");
  expect(heading.querySelector(".dialog-description")!.textContent).toBe("Paste the link.");
  expect(d.el.querySelector(".dialog-head > .tile")).not.toBeNull();
});

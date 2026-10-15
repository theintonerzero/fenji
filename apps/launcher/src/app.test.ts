import { expect, test } from "vitest";

import { mount } from "./app";
import { golden } from "./test-golden";
import type { Intent, Snapshot } from "./types";

test("the dialog a snapshot asks for comes over the screen: a link's other key before an update", async () => {
  const root = document.createElement("div");
  document.body.replaceChildren(root);
  const sent: Intent[] = [];
  let push: (s: Snapshot) => void = () => {};
  await mount(root, {
    snapshot: async () => golden("update-ready"),
    onSnapshot: async (cb) => {
      push = cb;
      return () => {};
    },
    send: async (intent) => {
      sent.push(intent);
      return null;
    },
  });
  expect(root.querySelector('[role="dialog"]')!.textContent).toContain("Fenji launcher 0.5.1 is ready");
  const both = golden("servers-key-change");
  both.update = golden("update-ready").update;
  push(both);
  expect(root.querySelectorAll('[role="dialog"]')).toHaveLength(1);
  expect(root.querySelector('[role="dialog"]')!.textContent).toContain("A different key for Ember Peak");
  push(golden("play-ready"));
  expect(root.querySelector('[role="dialog"]')).toBeNull();
  expect(root.querySelector("button.play")).not.toBeNull();
  expect(sent).toEqual([{ type: "viewTab", tab: "play" }]);
});

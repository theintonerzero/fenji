import { expect, test } from "vitest";

import { golden, mountPopover, press, settle } from "../screens/test-kit";
import { accountPopover } from "./account";

test("who the player is on each server: Sign out on the active one, Sign in on the others", async () => {
  const { root, sent, asked } = mountPopover(accountPopover, golden("servers"));
  const sections = [...root.querySelectorAll(".sec")].map((s) => s.textContent);
  expect(sections).toEqual([
    "MoonwardSigned in as lin_yueSign out",
    "Jade HallNot signed in · Sign in",
    "Ember PeakNot signed in · Sign in",
    "To delete an account, ask the host.",
  ]);
  press(root, "Sign out");
  expect(sent).toEqual([{ type: "signOut" }]);
  [...root.querySelectorAll("button")].filter((b) => b.textContent === "Sign in")[0]!.click();
  await settle();
  expect(asked.opened).toEqual([null]);
  expect(sent.at(-1)).toEqual({ type: "selectServer", id: 2 });
  expect(asked.signIns).toEqual([true]);
});

test("an invite waiting on a server offers its account", async () => {
  const s = golden("servers");
  s.pendingInvite = { serverId: 3, code: "K7QM-3XRD-VN2P-HT8B" };
  const { root, sent, asked } = mountPopover(accountPopover, s);
  expect(root.querySelectorAll(".sec")[2]!.textContent).toBe("Ember PeakInvited · Create account");
  press(root, "Create account");
  await settle();
  expect(sent).toEqual([{ type: "selectServer", id: 3 }]);
  expect(asked.signIns).toEqual([]);
});

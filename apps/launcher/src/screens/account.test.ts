import { expect, test } from "vitest";

import { account } from "./account";
import { golden, mount, press } from "./test-kit";

test("it says who is signed in where, and Sign out signs out", () => {
  const { root, sent } = mount(account, golden("account"), undefined, "account");
  expect(root.textContent).toContain("Signed in to Moonward as lin_yue.");
  expect(root.textContent).toContain("on Moonward · moonward.example.net");
  press(root, "Sign out");
  expect(sent).toEqual([{ type: "signOut" }]);
});

test("the note names the host, and the privacy page opens outside the launcher", () => {
  const { root, sent } = mount(account, golden("account"), undefined, "account");
  expect(root.textContent).toContain("Want your account deleted? Ask Wen, who hosts Moonward.");
  press(root, "privacy page");
  expect(sent).toEqual([{ type: "openUrl", url: "https://github.com/theintonerzero/fenji/blob/main/PRIVACY.md" }]);
});

test("without a host's name, the note still says who deletes accounts", () => {
  const s = golden("account");
  s.servers[0]!.host = null;
  const { root } = mount(account, s, undefined, "account");
  expect(root.textContent).toContain("Want your account deleted? Ask whoever hosts Moonward.");
});

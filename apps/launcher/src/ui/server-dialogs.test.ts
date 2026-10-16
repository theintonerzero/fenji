import { expect, test } from "vitest";

import { golden, recording, settle } from "../screens/test-kit";
import { addDialog, removeDialog } from "./server-dialogs";

test("Add a server: an address or an invite link, its refusal under the field, and it closes once added", async () => {
  let answer: string | null = "That isn't an invite link or an address.";
  const { ctx, asked } = recording((i) => (i.type === "addServer" ? answer : null));
  let done = 0;
  const d = addDialog(ctx, () => (done += 1));
  document.body.replaceChildren(d.el);
  expect(d.el.querySelector(".dialog-title")!.textContent).toBe("Add a server");
  expect(d.el.querySelector("label")!.textContent).toBe("Address or invite link");
  const input = d.el.querySelector("input")!;
  input.value = "jade hall";
  (d.el.querySelector("form") as HTMLFormElement).requestSubmit();
  await settle();
  expect(input.getAttribute("aria-invalid")).toBe("true");
  expect(done).toBe(0);
  answer = null;
  input.value = "jadehall.example.org";
  (d.el.querySelector("form") as HTMLFormElement).requestSubmit();
  await settle();
  expect(done).toBe(1);
  expect(asked.sent).toEqual([
    { type: "addServer", text: "jade hall" },
    { type: "addServer", text: "jadehall.example.org" },
  ]);
});

test("Cancel takes a refusal away with the dialog", async () => {
  const { ctx, asked } = recording(() => "That isn't an invite link or an address.");
  const d = addDialog(ctx, () => {});
  document.body.replaceChildren(d.el);
  d.el.querySelector("input")!.value = "nonsense";
  (d.el.querySelector("form") as HTMLFormElement).requestSubmit();
  await settle();
  [...d.el.querySelectorAll("button")].find((b) => b.textContent === "Cancel")!.click();
  expect(asked.sent.at(-1)).toEqual({ type: "dismissNotice", text: "That isn't an invite link or an address." });
});

test("Remove asks first, and says the account stays", () => {
  const { ctx, asked } = recording();
  const v = golden("servers").servers[1]!;
  const d = removeDialog(ctx, v, () => {});
  document.body.replaceChildren(d.el);
  expect(d.el.querySelector(".dialog-title")!.textContent).toBe("Remove Jade Hall?");
  expect(d.el.textContent).toContain("Your account on it stays.");
  [...d.el.querySelectorAll("button")].find((b) => b.textContent === "Remove")!.click();
  expect(asked.sent).toEqual([{ type: "removeServer", id: 2 }]);
});

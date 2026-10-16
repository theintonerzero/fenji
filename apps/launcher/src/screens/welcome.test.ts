import { expect, test } from "vitest";

import { embers, golden, mount, paste, press, settle } from "./test-kit";
import { welcome } from "./welcome";

const NEED_LINK = "Paste the whole link your host sent: a code alone doesn't say which server it's for.";

function submit(root: HTMLElement, text: string): void {
  root.querySelector("input")!.value = text;
  (root.querySelector("form") as HTMLFormElement).requestSubmit();
}

test("the invite goes to the model, and a refusal shows under the field", async () => {
  const { root, sent } = mount(welcome, golden("welcome"), (i) => (i.type === "addServer" ? NEED_LINK : null));
  expect(root.querySelector("label")!.textContent).toBe("Invite link or code");
  expect(embers(root)).toBe(1);
  submit(root, "K7QM-3XRD-VN2P-HT8B");
  expect(sent[0]).toEqual({ type: "addServer", text: "K7QM-3XRD-VN2P-HT8B" });
  await settle();
  const input = root.querySelector("input")!;
  expect(input.getAttribute("aria-invalid")).toBe("true");
  expect(root.querySelector(`#${input.getAttribute("aria-describedby")}`)!.textContent).toBe(NEED_LINK);
  expect(sent.at(-1)).toEqual({ type: "dismissNotice", text: NEED_LINK });
});

test("a link pasted from a wrapped line goes to the model as one", async () => {
  // #359: the model refuses a space inside a host, and a one-line field keeps a wrap's indent.
  const { root, sent } = mount(welcome, golden("welcome"));
  paste(root.querySelector("input")!, "https://moonward.example.\n   net/#invite=K7QM-3XRD-VN2P-HT8B");
  (root.querySelector("form") as HTMLFormElement).requestSubmit();
  await settle();
  expect(sent[0]).toEqual({ type: "addServer", text: "https://moonward.example.net/#invite=K7QM-3XRD-VN2P-HT8B" });
});

test("I already have an account asks for the address, then goes to Sign in", async () => {
  const { root, sent, tabs } = mount(welcome, golden("welcome"));
  press(root, "I already have an account");
  expect(root.querySelector("label")!.textContent).toBe("Server address");
  submit(root, "moonward.example.net");
  await settle();
  expect(sent).toEqual([{ type: "addServer", text: "moonward.example.net" }]);
  expect(tabs).toEqual(["account"]);
});

test("an empty field isn't sent", () => {
  const { root, sent } = mount(welcome, golden("welcome"));
  submit(root, "   ");
  expect(sent).toEqual([]);
  expect(root.querySelector("input")!.getAttribute("aria-invalid")).toBe("true");
});

test("it says what Fenji needs and never asks for", () => {
  const { root } = mount(welcome, golden("welcome"));
  expect(root.textContent).toContain("Welcome to Fenji");
  expect(root.textContent).toContain("No server yet");
  expect(root.textContent).toContain("Fenji never asks for your Steam or Gameforge password.");
  expect(root.textContent).toContain("Not set. Paste the invite your host sent you.");
});

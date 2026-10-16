import { expect, test } from "vitest";

import { signIn } from "./sign-in";
import { embers, golden, mount, press, settle } from "./test-kit";

function fill(root: HTMLElement, username = "lin_yue", password = "a long password"): void {
  const [user, pass] = root.querySelectorAll<HTMLInputElement>('input:not([type="checkbox"])');
  user!.value = username;
  pass!.value = password;
  (root.querySelector("form") as HTMLFormElement).requestSubmit();
}

test("remember defaults to true, and Sign in sends it", () => {
  const { root, sent } = mount(signIn, golden("play-signed-out"), undefined, "account");
  expect(root.querySelector("h1")!.textContent).toBe("Sign in to Moonward");
  expect(root.querySelector<HTMLInputElement>('input[type="checkbox"]')!.checked).toBe(true);
  expect(embers(root)).toBe(1);
  fill(root);
  expect(sent).toEqual([{ type: "signIn", username: "lin_yue", password: "a long password", remember: true }]);
});

test("the username the server remembers is filled in", () => {
  const { root } = mount(signIn, golden("play-signed-out"), undefined, "account");
  expect(root.querySelector<HTMLInputElement>('input[autocomplete="username"]')!.value).toBe("lin_yue");
});

test("needSignIn shows Cancel, which cancels", () => {
  const { root, sent } = mount(signIn, golden("sign-in"));
  press(root, "Cancel");
  expect(sent).toEqual([{ type: "cancelSignIn" }]);
  expect(mount(signIn, golden("play-signed-out")).root.textContent).not.toContain("Cancel");
});

test("a refusal shows under the form", async () => {
  const words = "That username and password don't match an account on Moonward.";
  const { root, sent } = mount(signIn, golden("sign-in"), (i) => (i.type === "signIn" ? words : null));
  fill(root);
  await settle();
  expect(root.querySelector(".form-error")!.textContent).toBe(words);
  expect(sent.at(-1)).toEqual({ type: "dismissNotice", text: words });
});

test("an empty password isn't sent", () => {
  const { root, sent } = mount(signIn, golden("sign-in"));
  fill(root, "lin_yue", "");
  expect(sent).toEqual([]);
  expect(root.textContent).toContain("Type your password.");
});

test("Have an invite? Create an account goes back to the invite field", () => {
  const { root, sent } = mount(signIn, golden("sign-in"));
  press(root, "Have an invite? Create an account");
  expect(root.querySelector("label")!.textContent).toBe("Invite link or code");
  const input = root.querySelector("input")!;
  input.value = "https://moonward.example.net/invite#K7QM-3XRD-VN2P-HT8B";
  (root.querySelector("form") as HTMLFormElement).requestSubmit();
  expect(sent).toEqual([{ type: "addServer", text: "https://moonward.example.net/invite#K7QM-3XRD-VN2P-HT8B" }]);
  press(root, "I already have an account");
  expect(root.querySelector("h1")!.textContent).toBe("Sign in to Moonward");
});

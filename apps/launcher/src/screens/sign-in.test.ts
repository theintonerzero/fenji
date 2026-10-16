import { expect, test } from "vitest";

import { signIn } from "./sign-in";
import { embers, golden, mount, press, settle } from "./test-kit";

function fill(root: HTMLElement, username = "lin_yue", password = "a long password"): void {
  const [user, pass] = root.querySelectorAll<HTMLInputElement>('input:not([type="checkbox"])');
  user!.value = username;
  pass!.value = password;
  (root.querySelector("form") as HTMLFormElement).requestSubmit();
}

test("the server's account, not Steam; remember defaults to true, and Sign in sends it", async () => {
  const { root, sent, asked } = mount(signIn, golden("play-signed-out"));
  expect(root.querySelector("h1")!.textContent).toBe("Sign in to Moonward");
  expect(root.querySelector(".lede")!.textContent).toBe("Your Moonward account, not Steam.");
  expect(root.querySelector<HTMLInputElement>('input[type="checkbox"]')!.checked).toBe(true);
  expect(embers(root)).toBe(1);
  fill(root);
  expect(sent).toEqual([{ type: "signIn", username: "lin_yue", password: "a long password", remember: true }]);
  await settle();
  expect(asked.signIns).toEqual([false]);
});

test("the username the server remembers is filled in", () => {
  const { root } = mount(signIn, golden("play-signed-out"));
  expect(root.querySelector<HTMLInputElement>('input[autocomplete="username"]')!.value).toBe("lin_yue");
});

test("Cancel cancels the sign-in Play waits for, and goes back", () => {
  const { root, sent, asked } = mount(signIn, golden("sign-in"));
  press(root, "Cancel");
  expect(sent).toEqual([{ type: "cancelSignIn" }]);
  expect(asked.signIns).toEqual([false]);
  const asking = mount(signIn, golden("play-signed-out"));
  press(asking.root, "Cancel");
  expect(asking.sent).toEqual([]);
  expect(asking.asked.signIns).toEqual([false]);
});

test("a refusal shows under the form", async () => {
  const words = "Wrong username or password for Moonward.";
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

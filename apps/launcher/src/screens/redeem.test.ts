import { expect, test } from "vitest";

import { redeem } from "./redeem";
import { embers, golden, mount, press, settle } from "./test-kit";

test("the form checks itself before it sends, then sends the redemption", async () => {
  const { root, sent } = mount(redeem, golden("redeem"));
  const [user, pass, again] = root.querySelectorAll("input");
  const submit = () => (root.querySelector("form") as HTMLFormElement).requestSubmit();
  user!.value = "li";
  pass!.value = "short";
  again!.value = "short";
  submit();
  expect(sent).toEqual([]);
  expect(root.textContent).toContain("Usernames are 3 to 16 letters, digits, _ or -.");
  user!.value = "lin_yue";
  pass!.value = "a long password";
  again!.value = "a long passwort";
  submit();
  expect(root.textContent).toContain("The passwords don't match.");
  expect(root.textContent).not.toContain("Usernames are 3 to 16 letters, digits, _ or -.");
  again!.value = "a long password";
  submit();
  await Promise.resolve();
  expect(sent).toEqual([{ type: "redeem", code: null, username: "lin_yue", password: "a long password" }]);
  expect(root.querySelector('input[type="password"]')).not.toBeNull();
});

test("it names the server, the host and the invite, with one ember action", () => {
  const { root } = mount(redeem, golden("redeem"));
  expect(root.textContent).toContain("Wen invited you to Moonward. Choose a username and a password.");
  expect(root.textContent).toContain("Invite recognised");
  expect(root.textContent).toContain("K7QM-3XRD-VN2P-HT8B");
  expect(embers(root)).toBe(1);
});

function fill(root: HTMLElement): void {
  const [user, pass, again] = root.querySelectorAll("input");
  user!.value = "lin_yue";
  pass!.value = "a long password";
  again!.value = "a long password";
  (root.querySelector("form") as HTMLFormElement).requestSubmit();
}

test("a taken name shows under Username, and the rest under the form", async () => {
  const taken = "“lin_yue” is taken on Moonward. Try another.";
  const spent = "That invite is used or expired. Ask Wen for a new one.";
  let answer = taken;
  const { root, sent } = mount(redeem, golden("redeem"), (i) => (i.type === "redeem" ? answer : null));
  fill(root);
  await settle();
  const username = root.querySelector('input[autocomplete="username"]')!;
  expect(username.getAttribute("aria-invalid")).toBe("true");
  expect(root.querySelector(`#${username.getAttribute("aria-describedby")}`)!.textContent).toBe(taken);
  expect(root.querySelector(".form-error")!.hasAttribute("hidden")).toBe(true);
  answer = spent;
  fill(root);
  await settle();
  expect(username.hasAttribute("aria-invalid")).toBe(false);
  expect(root.querySelector(".form-error")!.textContent).toBe(spent);
  expect(sent.filter((i) => i.type === "dismissNotice")).toHaveLength(2);
});

test("I already have an account cancels and shows Sign in, which can come back", () => {
  const { root, sent, tabs } = mount(redeem, golden("redeem"));
  press(root, "I already have an account");
  expect(sent).toEqual([{ type: "cancelSignIn" }]);
  expect(tabs).toEqual(["account"]);
  expect(root.querySelector("h1")!.textContent).toBe("Sign in to Moonward");
  press(root, "Have an invite? Create an account");
  expect(root.querySelector("h1")!.textContent).toBe("Create your account");
  expect(root.textContent).toContain("K7QM-3XRD-VN2P-HT8B");
});

import { expect, test } from "vitest";

import { redeem } from "./redeem";
import { embers, golden, mount, press, settle } from "./test-kit";

function fill(root: HTMLElement, username = "lin_yue", password = "a long password"): void {
  const [user, pass] = root.querySelectorAll("input");
  user!.value = username;
  pass!.value = password;
  (root.querySelector("form") as HTMLFormElement).requestSubmit();
}

test("Join names the server and its host, with one ember action", () => {
  const { root } = mount(redeem, golden("redeem"));
  expect(root.querySelector("h1")!.textContent).toBe("Join Moonward");
  expect(root.querySelector(".lede")!.textContent).toBe("Hosted by Wen.");
  expect(root.textContent).toContain("Not your Steam password.");
  expect(embers(root)).toBe(1);
});

test("the form checks itself before it sends, then sends the redemption", async () => {
  const { root, sent } = mount(redeem, golden("redeem"));
  fill(root, "li", "short");
  expect(sent).toEqual([]);
  expect(root.textContent).toContain("Usernames are 3 to 16 letters, digits, _ or -.");
  expect(root.textContent).toContain("Passwords are 10 to 128 characters.");
  fill(root);
  await Promise.resolve();
  expect(sent).toEqual([{ type: "redeem", code: null, username: "lin_yue", password: "a long password" }]);
});

test("Show reveals the one password as it's typed, to catch a typo", () => {
  const { root } = mount(redeem, golden("redeem"));
  const pass = root.querySelector<HTMLInputElement>('input[autocomplete="new-password"]')!;
  const show = root.querySelector<HTMLButtonElement>('button[aria-label="Show password"]')!;
  expect(pass.type).toBe("password");
  show.click();
  expect(pass.type).toBe("text");
  expect(show.getAttribute("aria-pressed")).toBe("true");
  show.click();
  expect(pass.type).toBe("password");
});

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
  answer = spent;
  fill(root);
  await settle();
  expect(username.hasAttribute("aria-invalid")).toBe(false);
  expect(root.querySelector(".form-error")!.textContent).toBe(spent);
  expect(sent.filter((i) => i.type === "dismissNotice")).toHaveLength(2);
});

test("I have an account signs in there instead", () => {
  const { root, asked } = mount(redeem, golden("redeem"));
  press(root, "I have an account");
  expect(asked.signIns).toEqual([true]);
});

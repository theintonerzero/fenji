import { expect, test } from "vitest";

import { embers, golden, mount, paste, press, settle } from "./test-kit";
import { welcome } from "./welcome";

const NEED_LINK = "Paste the whole link: a code alone doesn't say which server.";

function submit(root: HTMLElement, text: string): void {
  root.querySelector("input")!.value = text;
  (root.querySelector("form") as HTMLFormElement).requestSubmit();
}

test("the stage asks for the invite link, and a refusal shows under the field", async () => {
  const { root, sent } = mount(welcome, golden("welcome"), (i) => (i.type === "addServer" ? NEED_LINK : null));
  expect(root.querySelector("h1")!.textContent).toBe("Welcome to Fenji");
  expect(root.querySelector(".lede")!.textContent).toBe("Paste the invite link your host sent you.");
  // The field is named for assistive technology, its words the lede's.
  expect(root.querySelector("label")!.textContent).toBe("Invite link");
  expect(root.querySelector("label")!.classList.contains("visually-hidden")).toBe(true);
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

test("I have an account asks for the address, then goes on to sign in", async () => {
  const { root, sent, asked } = mount(welcome, golden("welcome"));
  press(root, "I have an account");
  expect(root.querySelector("label")!.textContent).toBe("Server address");
  expect(root.querySelector("input")!.placeholder).toBe("play.example.net");
  submit(root, "moonward.example.net");
  await settle();
  expect(sent).toEqual([{ type: "addServer", text: "moonward.example.net" }]);
  expect(asked.signIns).toEqual([true]);
  press(root, "I have an invite");
  expect(root.querySelector("label")!.textContent).toBe("Invite link");
});

test("an empty field isn't sent", () => {
  const { root, sent } = mount(welcome, golden("welcome"));
  submit(root, "   ");
  expect(sent).toEqual([]);
  expect(root.querySelector("input")!.getAttribute("aria-invalid")).toBe("true");
});

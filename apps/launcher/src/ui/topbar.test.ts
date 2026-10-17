import { expect, test } from "vitest";

import { golden, recording } from "../screens/test-kit";
import { topBar } from "./topbar";

function mounted() {
  const { ctx, asked } = recording();
  const bar = topBar(ctx);
  document.body.replaceChildren(bar.el);
  return { bar, asked, find: (label: string) => bar.el.querySelector<HTMLButtonElement>(`button[aria-label^="${label}"]`)! };
}

test("on the stage: the address, the Game chip, the account and Settings, and the window controls", () => {
  const { bar, find, asked } = mounted();
  bar.update(golden("play-ready"), "stage", null);
  expect(bar.el.querySelector(".address")!.textContent).toBe("moonward.example.net");
  expect(find("Game").textContent).toBe("Game ready");
  expect(find("Game").getAttribute("aria-label")).toBe("Game: Game ready");
  expect(find("Game").dataset.tone).toBe("ok");
  expect(find("Account").textContent).toBe("LY");
  expect(find("Account").getAttribute("aria-label")).toBe("Account: lin_yue");
  find("Game").click();
  find("Account").click();
  find("Settings").click();
  find("Minimise").click();
  find("Close").click();
  expect(asked.toggled).toEqual(["game", "account", "settings"]);
  expect(asked.sent).toEqual([{ type: "minimize" }, { type: "close" }]);
  expect(bar.el.hasAttribute("data-tauri-drag-region")).toBe(true);
});

test("each screen shows the controls its board does", () => {
  const { bar, find } = mounted();
  bar.update(golden("welcome"), "welcome", null);
  expect([find("Game").hidden, find("Account").hidden, find("Settings").hidden]).toEqual([true, true, false]);
  expect(bar.el.querySelector(".address")!.textContent).toBe("");
  bar.update(golden("redeem"), "redeem", null);
  expect([find("Game").hidden, find("Account").hidden]).toEqual([true, true]);
  bar.update(golden("sign-in"), "signIn", null);
  expect([find("Game").hidden, find("Account").hidden]).toEqual([false, true]);
});

test("the open popover's control shows open; signed out, the account is a figure", () => {
  const { bar, find } = mounted();
  bar.update(golden("play-signed-out"), "stage", "game");
  expect(find("Game").classList.contains("open")).toBe(true);
  expect(find("Game").getAttribute("aria-expanded")).toBe("true");
  expect(find("Settings").getAttribute("aria-expanded")).toBe("false");
  expect(find("Account").getAttribute("aria-label")).toBe("Account");
  expect(find("Account").querySelector("svg")).not.toBeNull();
});

test("after the address, the Fenji the server runs, from its last answer, and nothing when it never said", () => {
  const { bar } = mounted();
  // In the address's own group, so the bar's gap never falls between them (#409's review).
  const version = () => bar.el.querySelector(".top > .where > .address + .server-version")!.textContent;
  bar.update(golden("play-ready"), "stage", null);
  expect(version()).toBe(" · Fenji 0.5.0");
  // Offline, the version its last answer gave, as the address stays.
  bar.update(golden("play-offline"), "stage", null);
  expect(version()).toBe(" · Fenji 0.5.0");
  const unknown = golden("play-ready");
  unknown.servers[0]!.version = null;
  bar.update(unknown, "stage", null);
  expect(version()).toBe("");
  // No server's on the welcome screen.
  bar.update(golden("play-ready"), "welcome", null);
  expect(version()).toBe("");
});

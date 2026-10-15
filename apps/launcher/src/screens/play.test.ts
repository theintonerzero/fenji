import { expect, test } from "vitest";

import { play } from "./play";
import { embers, golden, mount, press } from "./test-kit";

test.each([
  "play-ready",
  "play-signed-out",
  "play-checking",
  "play-starting",
  "play-running",
  "play-maintenance",
  "play-offline",
  "play-key-changed",
  "play-unsupported",
  "play-ended",
])("%s has at most one ember action", (name) => {
  expect(embers(mount(play, golden(name)).root)).toBeLessThanOrEqual(1);
});

test("ready plays, and Play sends the intent", () => {
  const { root, sent } = mount(play, golden("play-ready"));
  (root.querySelector(".ember") as HTMLButtonElement).click();
  expect(sent).toContainEqual({ type: "play" });
  expect(root.textContent).toContain("Client verified · build 10015727 · 210 files");
});

test("every disabled Play says why", () => {
  for (const [name, why] of [
    ["play-checking", "Play opens when every file checks out"],
    ["play-maintenance", "Play opens when Wen ends maintenance"],
    ["play-unsupported", "Fenji can't start the client on this system yet"],
    ["play-running", "Running"],
    ["play-starting", "Starting"],
  ]) {
    const { root } = mount(play, golden(name!));
    expect(root.querySelector("button.play")!.hasAttribute("disabled")).toBe(true);
    expect(root.textContent).toContain(why);
  }
});

test("a hostile server shows its words as text and nothing more", () => {
  const s = golden("play-ready");
  const hostile = "<img src=x onerror=alert(1)>";
  s.servers[0]!.name = hostile;
  s.servers[0]!.motd = hostile;
  s.servers[0]!.host = hostile;
  s.lobby = [hostile];
  const { root } = mount(play, s);
  expect(root.querySelector("img")).toBeNull();
  expect(root.textContent).toContain(hostile);
});

test("the screen keeps its place across snapshots", () => {
  const { root, screen } = mount(play, golden("play-ready"));
  const button = root.querySelector("button.play");
  screen.update(golden("play-checking"));
  screen.update(golden("play-ready"));
  expect(root.querySelector("button.play")).toBe(button);
});

test("checking climbs the blade, and its one action stops the check", () => {
  const { root, sent } = mount(play, golden("play-checking"));
  expect(root.querySelector('[role="progressbar"]')!.getAttribute("aria-valuenow")).toBe("78");
  press(root, "Stop checking");
  expect(sent).toEqual([{ type: "stopCheck" }]);
});

test("ready offers a quick check beside Play", () => {
  const { root, sent } = mount(play, golden("play-ready"));
  press(root, "Check files");
  expect(sent).toEqual([{ type: "check", level: "quick" }]);
});

test("starting says the first start sets up Proton", () => {
  const { root } = mount(play, golden("play-starting"));
  expect(root.querySelector("button.play")!.textContent).toBe("Starting");
  expect(root.textContent).toContain("The first start sets up Proton, which takes a few minutes");
});

test("running says so, and who is signed in where", () => {
  const { root } = mount(play, golden("play-running"));
  expect(root.textContent).toContain("Swords of Legends Online is running");
  expect(root.textContent).toContain("Signed in to Moonward as lin_yue");
  expect(root.textContent).toContain("You · online");
});

test("maintenance closes Play until the host ends it, and checks again on request", () => {
  const { root, sent } = mount(play, golden("play-maintenance"));
  expect(root.textContent).toContain("Updating to Fenji 0.4.1. Back around 21:00.");
  expect(root.textContent).toContain("The lobby is closed while Moonward is in maintenance.");
  press(root, "Check again");
  expect(sent).toEqual([{ type: "refresh" }]);
});

test("offline lists what to try, and Try again is its one action", () => {
  const { root, sent, tabs } = mount(play, golden("play-offline"));
  expect(root.textContent).toContain("What to try");
  expect(root.textContent).toContain("Check the address: moonward.example.net.");
  // The words and the link inside a bullet flow as one line of text, not as columns.
  expect(root.querySelector(".bullets li")!.childNodes).toHaveLength(1);
  expect(root.textContent).toContain("Otherwise, check your internet connection, or ask Wen whether Moonward is up.");
  expect(root.textContent).toContain("Who's online shows again once Moonward answers.");
  expect(root.querySelector<HTMLButtonElement>("button.play")!.hidden).toBe(true);
  const [only] = root.querySelectorAll(".ember:not([disabled])");
  expect(only!.textContent).toBe("Try again");
  (only as HTMLButtonElement).click();
  expect(sent).toEqual([{ type: "refresh" }]);
  press(root, "Change it");
  expect(tabs).toEqual(["servers"]);
});

test("a changed key says so instead of the list", () => {
  const { root } = mount(play, golden("play-key-changed"));
  expect(root.textContent).toContain("Moonward's key has changed. Ask Wen for a new link.");
  expect(root.textContent).not.toContain("What to try");
});

test("a server's own key, with no pin, says to open the host's link", () => {
  const s = golden("play-key-changed");
  s.servers[0]!.status = { kind: "ownKey" };
  s.servers[0]!.host = null;
  const { root } = mount(play, s);
  expect(root.textContent).toContain("Moonward uses its own key. Open the link your host sent you.");
  expect(root.textContent).not.toContain("What to try");
});

test("unsupported uses exactly its words, and lists what still works", () => {
  const { root, tabs } = mount(play, golden("play-unsupported"));
  const why = root.querySelector("#play-unsupported");
  expect(why!.textContent).toBe("Fenji can't start the client on this system yet");
  expect(root.querySelector("button.play")!.getAttribute("aria-describedby")).toBe("play-unsupported");
  expect(root.textContent).toContain("Create your account");
  expect(root.textContent).toContain("Check your game folder");
  expect(root.textContent).toContain("See the server's state");
  press(root, "Client tab");
  press(root, "Servers tab");
  expect(tabs).toEqual(["client", "servers"]);
});

test("with no verified copy, Play points to the Client tab", () => {
  const s = golden("play-ready");
  s.client.folder = null;
  s.client.check = { kind: "none" };
  const { root, tabs } = mount(play, s);
  expect(root.querySelector("button.play")!.hasAttribute("disabled")).toBe(true);
  press(root, "Set up the game on the Client tab");
  expect(tabs).toEqual(["client"]);
});

test("signed out, the column asks for a sign-in to see who's playing", () => {
  const { root } = mount(play, golden("play-signed-out"));
  expect(root.textContent).toContain("Sign in to see who's playing.");
});

test("a notice shows above the buttons, and its close button dismisses it", () => {
  const { root, sent } = mount(play, golden("play-ended"));
  const notice = root.querySelector(".notice-card")!;
  expect(notice.textContent).toContain("The game couldn't start: umu-run ended with 3. The log folder has the details.");
  expect(notice.compareDocumentPosition(root.querySelector("button.play")!) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  press(root, "Open the log folder");
  (notice.querySelector('[aria-label="Dismiss"]') as HTMLButtonElement).click();
  expect(sent).toEqual([{ type: "openLogFolder" }, { type: "dismissNotice" }]);
});

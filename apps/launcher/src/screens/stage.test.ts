import { expect, test } from "vitest";

import { stage } from "./stage";
import { embers, golden, mount, press, shows } from "./test-kit";

const text = (root: HTMLElement, selector: string): string => root.querySelector(selector)?.textContent ?? "";
const play = (root: HTMLElement): HTMLButtonElement => root.querySelector("button.play")!;

test("ready: the name, one line of state, the host's notice, PLAY alight, and who's playing", () => {
  const { root, sent } = mount(stage, golden("play-ready"));
  expect(text(root, ".name")).toBe("Moonward");
  expect(text(root, ".state")).toBe("Online · 3/10 players · 24 ms · Germany");
  expect(text(root, ".host")).toBe("Wen: the lobby is open. Make your characters now; the world opens with Fenji 0.5.");
  expect(play(root).dataset.look).toBe("lit");
  expect(play(root).textContent).toBe("Play");
  expect(embers(root)).toBe(1);
  expect(text(root, ".friends")).toBe("WSKWen, Shu, Kai playing");
  expect(root.querySelector(".meta")!.hasAttribute("hidden")).toBe(true);
  play(root).click();
  expect(sent).toEqual([{ type: "play" }]);
});

test("verifying: PLAY kindles to the check's progress, and Stop is quiet words beside it", () => {
  const { root, sent, screen } = mount(stage, golden("play-checking"));
  const button = play(root);
  expect(button.dataset.look).toBe("kindle");
  expect(button.disabled).toBe(true);
  expect(button.textContent).toBe("Verifying 78%");
  expect(button.style.getPropertyValue("--p")).toBe("78%");
  expect(root.querySelector('[role="progressbar"]')!.getAttribute("aria-valuenow")).toBe("78");
  expect(embers(root)).toBe(0);
  expect(root.querySelector("button.quiet")!.textContent).toBe("Stop");
  // The progress streams in: PLAY and Stop stay the same elements, so focus stays.
  const s = golden("play-checking");
  if (s.job?.kind === "checking") {
    s.job.percent = 91;
  }
  screen.update(s);
  expect(play(root)).toBe(button);
  expect(button.textContent).toBe("Verifying 91%");
  press(root, "Stop");
  expect(sent).toEqual([{ type: "stopCheck" }]);
});

test("the moment the check ends, PLAY's fill completes and dissolves into the lit ember", () => {
  const { root, screen } = mount(stage, golden("play-checking"));
  const button = play(root);
  screen.update(golden("play-ready"));
  expect(button.dataset.look).toBe("lit");
  expect(button.classList.contains("kindled")).toBe(true);
  button.dispatchEvent(new Event("animationend"));
  expect(button.classList.contains("kindled")).toBe(false);
  // Lit from the start, or after a closed PLAY, there's nothing to complete.
  const fresh = mount(stage, golden("play-maintenance"));
  fresh.screen.update(golden("play-ready"));
  expect(play(fresh.root).classList.contains("kindled")).toBe(false);
});

test("starting: PLAY kindles with no progress to show, Cancel beside it, and the first start says why it's slow", () => {
  const { root, sent } = mount(stage, golden("play-starting"));
  expect(play(root).textContent).toBe("Starting");
  expect(play(root).classList.contains("drift")).toBe(true);
  expect(root.querySelector('[role="progressbar"]')!.hasAttribute("aria-valuenow")).toBe(false);
  expect(text(root, ".meta")).toBe("Setting up Proton (first start only)");
  press(root, "Cancel");
  expect(sent).toEqual([{ type: "cancelStart" }]);
});

test("starting while GE-Proton downloads: the meta line names it with its percent, and PLAY fills to it", () => {
  const s = golden("play-getting-proton");
  const { root, screen } = mount(stage, s);
  expect(play(root).textContent).toBe("Starting");
  expect(root.querySelector('[role="progressbar"]')!.getAttribute("aria-valuenow")).toBe("42");
  expect(text(root, ".meta")).toBe("Getting GE-Proton 42%");
  // Before its size is known, the name alone; umu-launcher, fetched once, says so.
  screen.update({ ...s, job: { kind: "fetching", tool: "GE-Proton", percent: null } });
  expect(text(root, ".meta")).toBe("Getting GE-Proton");
  screen.update({ ...s, job: { kind: "fetching", tool: "umu-launcher", percent: 30 } });
  expect(text(root, ".meta")).toBe("Getting umu-launcher (first start only)");
});

test("running: an outline with a dot, since when, and the player leads the friends", () => {
  const { root } = mount(stage, golden("play-running"));
  expect(play(root).dataset.look).toBe("running");
  expect(play(root).textContent).toBe("Running");
  expect(text(root, ".meta")).toMatch(/^Since \d\d:\d\d$/);
  expect(text(root, ".friends")).toBe("LYWSKYou, Wen, Shu, Kai playing");
  expect(embers(root)).toBe(0);
});

test("maintenance: the host's message, when they're back, PLAY closed and Refresh", () => {
  const { root, sent } = mount(stage, golden("play-maintenance"));
  expect(text(root, ".state")).toMatch(/^Maintenance · back around .* · Germany$/);
  expect(text(root, ".host")).toBe("Wen: Updating to Fenji 0.4.1. Back around 21:00.");
  expect(play(root).dataset.look).toBe("closed");
  expect(root.querySelector(".friends")).toBeNull();
  press(root, "Refresh");
  expect(sent).toEqual([{ type: "refresh" }]);
});

test("offline: the error mark, Retry, and What to try opens three things to check", () => {
  const { root, sent } = mount(stage, golden("play-offline"));
  expect(root.querySelector(".state .bang")).not.toBeNull();
  expect(text(root, ".state")).toMatch(/^Offline · last seen/);
  const list = root.querySelector(".try")!;
  expect(list.hasAttribute("hidden")).toBe(true);
  press(root, "What to try");
  expect(list.hasAttribute("hidden")).toBe(false);
  expect(list.querySelectorAll("li")).toHaveLength(3);
  expect(list.textContent).toContain("Check the address, moonward.example.net, with Wen.");
  press(root, "Retry");
  expect(sent).toEqual([{ type: "refresh" }]);
});

test("its key changed: why, in the host's words, and Paste new link opens Add a server", () => {
  const { root, asked } = mount(stage, golden("play-key-changed"));
  expect(text(root, ".state")).toBe("Key changed");
  expect(text(root, ".host")).toBe("This isn't the server you trusted. Ask Wen for a new link.");
  press(root, "Paste new link");
  expect(asked.adds).toBe(1);
});

test("off Linux: PLAY closed, and one line says so", () => {
  const { root } = mount(stage, golden("play-unsupported"));
  expect(play(root).dataset.look).toBe("closed");
  expect(text(root, ".meta")).toBe("Playing on Windows isn't supported yet.");
  expect(embers(root)).toBe(0);
  expect(shows(root, "Set up the game")).toBe(false);
});

test("a copy that isn't ready: PLAY closed, and the one action opens Game", () => {
  for (const [file, words] of [
    ["client-setup", "Set up the game"],
    ["client-repair", "Repair"],
  ] as const) {
    const { root, asked } = mount(stage, golden(file));
    expect(play(root).dataset.look).toBe("closed");
    press(root, words);
    expect(asked.opened).toEqual(["game"]);
  }
  // A download shows on the Game chip instead.
  const { root } = mount(stage, golden("client-downloading"));
  expect(root.querySelectorAll("button.btn:not([hidden]), button.quiet:not([hidden])")).toHaveLength(0);
});

test("a failed start: the model's words, Open logs, and Dismiss", () => {
  const { root, sent } = mount(stage, golden("play-ended"));
  const note = root.querySelector('.note[role="alert"]')!;
  expect(note.textContent).toContain("The game couldn't start (code 3). See the logs.");
  press(root, "Open logs");
  (note.querySelector('button[aria-label="Dismiss"]') as HTMLButtonElement).click();
  expect(sent).toEqual([
    { type: "openLogFolder" },
    { type: "dismissNotice", text: "The game couldn't start (code 3). See the logs." },
  ]);
});

test("a start that ends lit plays no kindled moment: that's for the end of a check", () => {
  const { root, screen } = mount(stage, golden("play-starting"));
  screen.update(golden("play-ready"));
  expect(play(root).classList.contains("kindled")).toBe(false);
});

test("the state line speaks on a change of state, not on every ping", () => {
  const { root, screen } = mount(stage, golden("play-ready"));
  const live = root.querySelector('.state [role="status"]')!;
  expect(live.textContent).toBe("Online · 3/10 players");
  expect(text(root, ".state")).toBe("Online · 3/10 players · 24 ms · Germany");
  const pinged = golden("play-ready");
  pinged.servers[0]!.pingMs = 31;
  const before = live.firstChild;
  screen.update(pinged);
  expect(live.firstChild).toBe(before);
  expect(text(root, ".state")).toBe("Online · 3/10 players · 31 ms · Germany");
});

test("the region its host set closes the line of state, and a server without one shows none (hosting spec Amendment C, HC5)", () => {
  const { root, screen } = mount(stage, golden("play-ready"));
  // Outside the live region, which speaks on a change of state: a region rarely changes.
  expect(root.querySelector('.state [role="status"]')!.textContent).toBe("Online · 3/10 players");
  // But read with the line: PLAY's description holds it, and nothing hides it from assistive
  // technology, as it does the ping.
  const region = root.querySelector(".state .region")!;
  expect(region.textContent).toBe(" · Germany");
  expect(region.getAttribute("aria-hidden")).toBeNull();
  const described = root.querySelector(`[id="${play(root).getAttribute("aria-describedby")}"]`)!;
  expect(described.contains(region)).toBe(true);
  const elsewhere = golden("play-ready");
  elsewhere.servers[0]!.region = null;
  screen.update(elsewhere);
  expect(text(root, ".state")).toBe("Online · 3/10 players · 24 ms");
  // Offline, the place it had at its last answer stays beside when that was.
  const { root: offline } = mount(stage, golden("play-offline"));
  expect(text(offline, ".state")).toMatch(/^Offline · last seen .* · Germany$/);
});

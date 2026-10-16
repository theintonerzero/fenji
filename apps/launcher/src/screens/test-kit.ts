// What the screens' tests share (launcher plan Task 13): a golden snapshot, a screen mounted on it
// with a recording `send`, and a count of the ember actions a player can press.
import type { Ctx, ScreenFactory } from "../app";
import type { Tab } from "../route";
import { golden as load } from "../test-golden";
import type { Intent, Snapshot } from "../types";

export const golden = (name: string): Snapshot => structuredClone(load(name));

/** Mounts a screen. `reply` answers each intent: `null` when it's done, or the refusal's words. */
export function mount(
  make: ScreenFactory,
  snap: Snapshot,
  reply: (intent: Intent) => string | null = () => null,
  tab: Tab = "play",
) {
  const sent: Intent[] = [];
  const tabs: Tab[] = [];
  const root = document.createElement("div");
  document.body.replaceChildren(root);
  const ctx: Ctx = {
    tab,
    setTab(next) {
      tabs.push(next);
      ctx.tab = next;
    },
    send: async (intent) => {
      sent.push(intent);
      return reply(intent);
    },
  };
  const screen = make(root, ctx);
  screen.update(snap);
  return { root, sent, screen, tabs };
}

export const embers = (root: HTMLElement): number => root.querySelectorAll(".ember:not([disabled])").length;

/** A button by its words. */
export function press(root: HTMLElement, words: string): void {
  const found = [...root.querySelectorAll("button")].find((b) => b.textContent?.trim() === words);
  if (!found) {
    throw new Error(`no button reads ${words}`);
  }
  found.click();
}

/** A paste of `text` into `input`, as the clipboard holds it, line breaks and all. */
export function paste(input: HTMLInputElement, text: string): Event {
  const event = new Event("paste", { bubbles: true, cancelable: true });
  Object.defineProperty(event, "clipboardData", {
    value: { getData: (type: string) => (type === "text/plain" ? text : "") },
  });
  input.dispatchEvent(event);
  return event;
}

/** Lets the screen's pending sends answer. */
export const settle = (): Promise<void> => new Promise((done) => setTimeout(done, 0));

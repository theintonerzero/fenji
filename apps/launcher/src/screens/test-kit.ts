// What the view's tests share: a golden snapshot, a screen or a popover mounted on it with a Ctx that
// records what it's asked, and a count of the ember actions a player can press.
import type { Ctx, PopoverContent, PopoverName, ScreenFactory } from "../app";
import { golden as load } from "../test-golden";
import type { Intent, ServerView, Snapshot } from "../types";
import type { Dialog } from "../ui/dialog";

export const golden = (name: string): Snapshot => structuredClone(load(name));

/** Everything a Ctx was asked, in order. */
export interface Asked {
  sent: Intent[];
  opened: (PopoverName | null)[];
  toggled: PopoverName[];
  selected: number[];
  signIns: (number | "added" | null)[];
  adds: number;
  removes: ServerView[];
  dialogs: Dialog[];
}

/** A Ctx that records. `reply` answers each intent: `null` when it's done, or the refusal's words. */
export function recording(reply: (intent: Intent) => string | null = () => null): { ctx: Ctx; asked: Asked } {
  const asked: Asked = { sent: [], opened: [], toggled: [], selected: [], signIns: [], adds: 0, removes: [], dialogs: [] };
  const ctx: Ctx = {
    send: async (intent) => {
      asked.sent.push(intent);
      return reply(intent);
    },
    open: (which) => void asked.opened.push(which),
    toggle: (which) => void asked.toggled.push(which),
    select: (id) => void asked.selected.push(id),
    signIn: (server) => void asked.signIns.push(server),
    addServer: () => void (asked.adds += 1),
    removeServer: (v) => void asked.removes.push(v),
    show: (make) => {
      const made = make(() => {});
      asked.dialogs.push(made);
      document.body.append(made.el);
    },
  };
  return { ctx, asked };
}

/** Mounts a screen on `snap`. */
export function mount(make: ScreenFactory, snap: Snapshot, reply?: (intent: Intent) => string | null) {
  const root = document.createElement("div");
  document.body.replaceChildren(root);
  const { ctx, asked } = recording(reply);
  const screen = make(root, ctx);
  screen.update(snap);
  return { root, sent: asked.sent, asked, screen };
}

/** A popover's body on `snap`, in the page. */
export function mountPopover(make: (ctx: Ctx) => PopoverContent, snap: Snapshot, reply?: (intent: Intent) => string | null) {
  const { ctx, asked } = recording(reply);
  const content = make(ctx);
  content.update(snap);
  document.body.replaceChildren(content.body);
  return { root: content.body, sent: asked.sent, asked, content };
}

export const embers = (root: ParentNode): number => root.querySelectorAll(".ember:not([disabled])").length;

/** A button by its words. */
export function press(root: ParentNode, words: string): void {
  const found = [...root.querySelectorAll("button")].find((b) => b.textContent?.trim() === words);
  if (!found) {
    throw new Error(`no button reads ${words}`);
  }
  found.click();
}

/** Whether a button with these words is there and shown. */
export function shows(root: ParentNode, words: string): boolean {
  return [...root.querySelectorAll("button")].some((b) => b.textContent?.trim() === words && !b.closest("[hidden]"));
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

/** Lets pending sends answer. */
export const settle = (): Promise<void> => new Promise((done) => setTimeout(done, 0));

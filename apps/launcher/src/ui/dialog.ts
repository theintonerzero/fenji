// A centred dialog (R7: Add a server, a changed key, Remove): 500 px over a scrim on the stage,
// role="dialog" with aria-modal, Escape to close, focus kept inside while it's open and given back
// when it closes, or to the dialog it closes onto. Actions sit bottom right: the secondary first,
// then the one primary.
import { h, type Child } from "../dom";

export interface DialogParts {
  title: string;
  description?: Child;
  body?: Child[];
  actions: HTMLElement[];
  /** Escape, or a click on the scrim. */
  onClose(): void;
}

export interface Dialog {
  el: HTMLElement;
  close(): void;
}

let next = 0;

const FOCUSABLE = 'button:not([disabled]), input:not([disabled]), a[href], [tabindex]:not([tabindex="-1"])';

/** The first control inside `box` a player can reach, or `box` itself. */
export function firstFocusable(box: HTMLElement): HTMLElement {
  return [...box.querySelectorAll<HTMLElement>(FOCUSABLE)].find((el) => !el.closest("[hidden]")) ?? box;
}

/** Keeps Tab inside `box`, and calls `onEscape` for Escape. */
export function trapFocus(box: HTMLElement, onEscape: () => void): void {
  box.addEventListener("keydown", (event) => {
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      onEscape();
      return;
    }
    if (event.key !== "Tab") {
      return;
    }
    const focusable = [...box.querySelectorAll<HTMLElement>(FOCUSABLE)].filter((el) => !el.closest("[hidden]"));
    const first = focusable[0];
    const last = focusable[focusable.length - 1];
    if (!first || !last) {
      event.preventDefault();
      return;
    }
    // The box itself (a popover's panel, focused as it opens) is the first stop for Shift+Tab.
    if (event.shiftKey && (document.activeElement === first || document.activeElement === box)) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault();
      first.focus();
    }
  });
}

export function dialog(parts: DialogParts): Dialog {
  next += 1;
  const titleId = `dialog-${next}-title`;
  const returnTo = document.activeElement instanceof HTMLElement ? document.activeElement : null;
  const box = h(
    "div",
    { class: "dialog", role: "dialog", "aria-modal": "true", "aria-labelledby": titleId, tabindex: "-1" },
    h("h2", { class: "dialog-title", id: titleId }, parts.title),
    parts.description === undefined ? null : h("p", { class: "dialog-description" }, parts.description),
    parts.body ? h("div", { class: "dialog-body" }, ...parts.body) : null,
    h("div", { class: "dialog-actions" }, ...parts.actions),
  );
  const el = h("div", { class: "scrim" }, box);
  el.addEventListener("mousedown", (event) => {
    if (event.target === el) {
      parts.onClose();
    }
  });
  trapFocus(box, parts.onClose);
  queueMicrotask(() => firstFocusable(box).focus());
  return {
    el,
    close() {
      el.remove();
      // Closed onto another open dialog, as Add can be onto a changed key's: that one keeps
      // focus, never this one's opener behind it (#376).
      const under = [...document.querySelectorAll<HTMLElement>('.dialog[aria-modal="true"]')].at(-1);
      if (under !== undefined) {
        if (!under.contains(document.activeElement)) {
          firstFocusable(under).focus();
        }
      } else if (returnTo?.isConnected) {
        returnTo.focus();
      }
    },
  };
}

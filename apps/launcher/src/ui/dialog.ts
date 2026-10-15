// A modal dialog (DESIGN.md, Dialogs): 480 px over the scrim, role="dialog" with aria-modal,
// Escape to close, focus kept inside while it's open and given back when it closes. Actions sit
// bottom right: secondary first, then the one primary.
import { h, type Child } from "../dom";

export interface DialogParts {
  title: string;
  icon?: Element;
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

export function dialog(parts: DialogParts): Dialog {
  next += 1;
  const titleId = `dialog-${next}-title`;
  const returnTo = document.activeElement instanceof HTMLElement ? document.activeElement : null;
  const box = h(
    "div",
    { class: "dialog", role: "dialog", "aria-modal": "true", "aria-labelledby": titleId, tabindex: "-1" },
    h(
      "div",
      { class: "dialog-head" },
      parts.icon ? h("span", { class: "tile" }, parts.icon) : null,
      h("h2", { class: "dialog-title", id: titleId }, parts.title),
    ),
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
  box.addEventListener("keydown", (event) => {
    if (event.key === "Escape") {
      event.preventDefault();
      parts.onClose();
      return;
    }
    if (event.key !== "Tab") {
      return;
    }
    const focusable = [...box.querySelectorAll<HTMLElement>(FOCUSABLE)];
    const first = focusable[0];
    const last = focusable[focusable.length - 1];
    if (!first || !last) {
      event.preventDefault();
      return;
    }
    if (event.shiftKey && document.activeElement === first) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault();
      first.focus();
    }
  });
  queueMicrotask(() => (box.querySelector<HTMLElement>(FOCUSABLE) ?? box).focus());
  return {
    el,
    close() {
      el.remove();
      returnTo?.focus();
    },
  };
}

// A labelled field (the boards): the label above, or only named where the task's words name it, and an
// error below that says what happened and what to try, with the circled exclamation.
import { h } from "../dom";
import { icons } from "./icons";

export interface Field {
  el: HTMLElement;
  /** The error under the field, or none. */
  setError(text: string | null): void;
  setLabel(text: string): void;
}

let next = 0;

export interface FieldOptions {
  /** The label shown above the field (the default), or only named for assistive technology. */
  visibleLabel?: boolean;
  /** A control inside the field's right edge: the password's Show. */
  trailing?: HTMLElement;
}

/** A field. A hint the input already names in `aria-describedby` stays named beside an error. */
export function field(label: string, input: HTMLInputElement, options: FieldOptions = {}): Field {
  next += 1;
  const id = input.id || `field-${next}`;
  const errorId = `${id}-error`;
  const hint = input.getAttribute("aria-describedby");
  input.id = id;
  input.classList.add("input");
  const words = h("span", {});
  const error = h("p", { class: "field-error", id: errorId, hidden: true }, icons.error(14), words);
  const name = h("label", { class: options.visibleLabel === false ? "visually-hidden" : "field-label", for: id }, label);
  const control = options.trailing ? h("div", { class: "field-control" }, input, options.trailing) : input;
  const el = h("div", { class: "field" }, name, control, error);
  return {
    el,
    setError(text) {
      if (text === null) {
        input.removeAttribute("aria-invalid");
        if (hint === null) {
          input.removeAttribute("aria-describedby");
        } else {
          input.setAttribute("aria-describedby", hint);
        }
        error.hidden = true;
        words.textContent = "";
      } else {
        input.setAttribute("aria-invalid", "true");
        input.setAttribute("aria-describedby", hint === null ? errorId : `${errorId} ${hint}`);
        words.textContent = text;
        error.hidden = false;
      }
    },
    setLabel(text) {
      name.textContent = text;
    },
  };
}

/** Show and hide a password as it's typed: with one password field, it's how a typo is caught. */
export function revealToggle(input: HTMLInputElement): HTMLButtonElement {
  const toggle = h("button", {
    class: "icon-button reveal",
    type: "button",
    "aria-label": "Show password",
    "aria-pressed": "false",
    onclick: () => {
      const showing = input.type === "text";
      input.type = showing ? "password" : "text";
      toggle.setAttribute("aria-pressed", String(!showing));
      toggle.replaceChildren(showing ? icons.eye(18) : icons.eyeOff(18));
    },
  });
  toggle.append(icons.eye(18));
  return toggle;
}

/** A checkbox (DESIGN.md, Checkbox): a 20 px box with the gold check, its label clickable. */
export function checkbox(label: string, checked: boolean): { el: HTMLLabelElement; input: HTMLInputElement } {
  const input = h("input", { class: "checkbox-input", type: "checkbox", checked });
  const el = h(
    "label",
    { class: "checkbox" },
    input,
    h("span", { class: "checkbox-box", "aria-hidden": "true" }, icons.check(12)),
    label,
  );
  return { el, input };
}

/**
 * Makes a link field join a wrapped link as it's pasted (#359). The host console wraps a long link
 * at the window's edge, and a one-line field turns a pasted line break into a space and keeps the
 * indent after it, which the model can't tell from a space typed inside a host name, and refuses.
 * In a link (text with `#` or `://`, the model's own test), each line break goes with the spaces
 * around it. Anything else is the field's own, as the model reads it: two lines that aren't a link
 * never join into a host neither shows.
 */
export function joinWrapsOnPaste(input: HTMLInputElement): void {
  input.addEventListener("paste", (event) => {
    const text = (event as ClipboardEvent).clipboardData?.getData("text/plain") ?? "";
    if (!/[\r\n]/.test(text) || !(text.includes("#") || text.includes("://"))) {
      return;
    }
    event.preventDefault();
    const joined = text.trim().replace(/\s*[\r\n]\s*/g, "");
    const start = input.selectionStart ?? input.value.length;
    input.setRangeText(joined, start, input.selectionEnd ?? start, "end");
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
}

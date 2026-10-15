// A labelled field (DESIGN.md, Inputs / Fields): the label always visible above, and an error
// below that says what happened and what to try, with the circled exclamation.
import { h } from "../dom";
import { icons } from "./icons";

export interface Field {
  el: HTMLElement;
  /** The error under the field, or none. */
  setError(text: string | null): void;
}

let next = 0;

export function field(label: string, input: HTMLInputElement): Field {
  next += 1;
  const id = input.id || `field-${next}`;
  const errorId = `${id}-error`;
  input.id = id;
  input.classList.add("input");
  const words = h("span", {});
  const error = h("p", { class: "field-error", id: errorId, hidden: true }, icons.error(14), words);
  const el = h("div", { class: "field" }, h("label", { class: "field-label", for: id }, label), input, error);
  return {
    el,
    setError(text) {
      if (text === null) {
        input.removeAttribute("aria-invalid");
        input.removeAttribute("aria-describedby");
        error.hidden = true;
        words.textContent = "";
      } else {
        input.setAttribute("aria-invalid", "true");
        input.setAttribute("aria-describedby", errorId);
        words.textContent = text;
        error.hidden = false;
      }
    },
  };
}

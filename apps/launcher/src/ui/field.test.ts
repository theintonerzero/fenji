import { expect, test } from "vitest";

import { h } from "../dom";
import { paste } from "../screens/test-kit";
import { joinWrapsOnPaste } from "./field";

function linkField(): HTMLInputElement {
  const input = h("input", { type: "text" });
  joinWrapsOnPaste(input);
  return input;
}

test("a link wrapped inside its host pastes as one, its break and indent gone", () => {
  // #359: a one-line field would keep the indent, which the model refuses as a typed space.
  const input = linkField();
  const event = paste(input, "https://ec2-203-0-113-5.eu-west-1.compute.\n   amazonaws.com:7801/#invite=K7QM-3XRD-VN2P-HT8B\n");
  expect(event.defaultPrevented).toBe(true);
  expect(input.value).toBe("https://ec2-203-0-113-5.eu-west-1.compute.amazonaws.com:7801/#invite=K7QM-3XRD-VN2P-HT8B");
});

test("a paste on one line is the field's own, spaces and all", () => {
  const input = linkField();
  expect(paste(input, "https://play example.net").defaultPrevented).toBe(false);
});

test("a wrapped paste replaces the selection, and the caret ends after it", () => {
  const input = linkField();
  const typed: string[] = [];
  input.addEventListener("input", () => typed.push(input.value));
  input.value = "[old]";
  input.setSelectionRange(0, 5);
  paste(input, "ABCD-EFGH-  \r\n\t JKMN-PQRS");
  expect(input.value).toBe("ABCD-EFGH-JKMN-PQRS");
  expect(input.selectionStart).toBe(19);
  expect(typed).toEqual(["ABCD-EFGH-JKMN-PQRS"]);
});

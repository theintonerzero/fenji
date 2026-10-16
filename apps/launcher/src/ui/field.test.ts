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

test("only a link's wrap is joined: other lines are the field's own, as the model reads them", () => {
  // Task 3's review: two lines that aren't a link would join into a host neither line shows. The
  // model joins a wrap only in a link (text with # or ://), and refuses an address with a space.
  const input = linkField();
  expect(paste(input, "play.example.net\nevil.example").defaultPrevented).toBe(false);
  expect(paste(input, "https://play.example.\n  net").defaultPrevented).toBe(true);
});

test("a wrapped paste replaces the selection, and the caret ends after it", () => {
  const input = linkField();
  const typed: string[] = [];
  input.addEventListener("input", () => typed.push(input.value));
  input.value = "[old]";
  input.setSelectionRange(0, 5);
  paste(input, "https://moonward.example.net/#invite=K7QM-  \r\n\t 3XRD-VN2P-HT8B");
  const joined = "https://moonward.example.net/#invite=K7QM-3XRD-VN2P-HT8B";
  expect(input.value).toBe(joined);
  expect(input.selectionStart).toBe(joined.length);
  expect(typed).toEqual([joined]);
});

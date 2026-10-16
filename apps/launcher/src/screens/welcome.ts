// Welcome (the redesign's Welcome board): no server yet, so the stage asks for the one thing the
// host sent, the invite link. "I have an account" asks for the server's address instead, then goes
// on to sign in there.
import type { Ctx, Screen } from "../app";
import { h } from "../dom";
import { button } from "../ui/button";
import { field, joinWrapsOnPaste } from "../ui/field";
import { noticeSlot, refused } from "./parts";

type Mode = "invite" | "address";

const WORDS: Record<Mode, { lede: string; label: string; placeholder: string; other: string; empty: string }> = {
  invite: {
    lede: "Paste the invite link your host sent you.",
    label: "Invite link",
    placeholder: "https://play.example.net/#invite=…",
    other: "I have an account",
    empty: "Paste the invite link first.",
  },
  address: {
    lede: "Type your server's address, then sign in.",
    label: "Server address",
    placeholder: "play.example.net",
    other: "I have an invite",
    empty: "Type the address first.",
  },
};

export function welcome(root: HTMLElement, ctx: Ctx): Screen {
  let mode: Mode = "invite";
  const slot = noticeSlot(ctx);
  const lede = h("p", { class: "lede" });
  const input = h("input", { type: "text", class: "mono", autocomplete: "off", spellcheck: "false" });
  joinWrapsOnPaste(input);
  const box = field(WORDS.invite.label, input, { visibleLabel: false });
  const go = button.primary("Continue");
  go.type = "submit";
  const other = button.link("", () => {
    setMode(mode === "invite" ? "address" : "invite");
    input.focus();
  });
  const submit = async (): Promise<void> => {
    const text = input.value.trim();
    if (text === "") {
      box.setError(WORDS[mode].empty);
      input.focus();
      return;
    }
    box.setError(null);
    go.disabled = true;
    slot.hold(true);
    const asked = mode;
    const refusal = await ctx.send({ type: "addServer", text });
    go.disabled = false;
    slot.hold(false);
    if (refusal === null) {
      // An invite shows Join by itself; an address goes on to sign in.
      if (asked === "address") {
        ctx.signIn("added");
      }
      return;
    }
    refused(ctx, slot, refusal, (words) => box.setError(words));
    input.focus();
  };
  const setMode = (to: Mode): void => {
    mode = to;
    const words = WORDS[to];
    lede.textContent = words.lede;
    box.setLabel(words.label);
    box.setError(null);
    input.placeholder = words.placeholder;
    other.textContent = words.other;
  };
  root.append(
    h(
      "div",
      { class: "task" },
      h("h1", { class: "task-title" }, "Welcome to Fenji"),
      lede,
      slot.el,
      h(
        "form",
        {
          class: "task-form",
          novalidate: true,
          onsubmit: (event) => {
            event.preventDefault();
            void submit();
          },
        },
        box.el,
        h("div", { class: "actions" }, go, other),
      ),
    ),
  );
  setMode("invite");
  return {
    update(s) {
      slot.update(s);
    },
    destroy() {
      root.replaceChildren();
    },
  };
}

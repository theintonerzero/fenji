// The rail's dialogs (R7; the Add a server board): Add a server, from + or from a stage whose
// server needs a new link, and Remove, from a seal's menu.
import type { Ctx } from "../app";
import { h } from "../dom";
import type { ServerView } from "../types";
import { button } from "./button";
import { dialog, type Dialog } from "./dialog";
import { field, joinWrapsOnPaste } from "./field";

/** An address or an invite link, nothing else; a refusal shows under the field. */
export function addDialog(ctx: Ctx, done: () => void): Dialog {
  // The refusal shown under the field, which waited as the notice too.
  let refusedHere: string | null = null;
  const input = h("input", {
    type: "text",
    class: "mono",
    autocomplete: "off",
    spellcheck: "false",
    placeholder: "play.example.net",
  });
  joinWrapsOnPaste(input);
  const box = field("Address or invite link", input);
  const add = button.primary("Add");
  add.type = "submit";
  add.setAttribute("form", "add-server");
  const close = (): void => {
    shown.close();
    done();
    // A refusal waited as the notice as well as under the field: it goes with the dialog.
    if (refusedHere !== null) {
      void ctx.send({ type: "dismissNotice", text: refusedHere });
    }
  };
  const submit = async (): Promise<void> => {
    const text = input.value.trim();
    if (text === "") {
      box.setError("Paste an invite link or an address.");
      input.focus();
      return;
    }
    add.disabled = true;
    const refusal = await ctx.send({ type: "addServer", text });
    add.disabled = false;
    if (refusal === null) {
      refusedHere = null;
      close();
      return;
    }
    refusedHere = refusal;
    box.setError(refusal);
    input.focus();
  };
  const shown = dialog({
    title: "Add a server",
    body: [
      h(
        "form",
        {
          id: "add-server",
          novalidate: true,
          onsubmit: (event) => {
            event.preventDefault();
            void submit();
          },
        },
        box.el,
      ),
    ],
    actions: [button.secondary("Cancel", close), add],
    onClose: close,
  });
  return { el: shown.el, close };
}

/** "Remove Moonward? Your account on it stays." */
export function removeDialog(ctx: Ctx, v: ServerView, done: () => void): Dialog {
  const close = (): void => {
    shown.close();
    done();
  };
  const shown = dialog({
    title: `Remove ${v.name}?`,
    description: "Your account on it stays. To add it again, you'll need its link or address.",
    actions: [
      button.secondary("Cancel", close),
      button.primary("Remove", () => {
        void ctx.send({ type: "removeServer", id: v.id });
        close();
      }),
    ],
    onClose: close,
  });
  return { el: shown.el, close };
}

// Welcome (launcher spec §3.2; the Welcome board): the first run, with no server saved. Its field
// takes the invite the host sent (AddServer); "I already have an account" asks for the server's
// address instead, then goes on to Sign in. Sign in borrows the same form (inviteForm) for a
// player who has an invite after all.
import type { Ctx, Screen } from "../app";
import { h } from "../dom";
import { bladeFor, groundFor } from "../route";
import { blade } from "../ui/blade";
import { button } from "../ui/button";
import { cardSection } from "../ui/column";
import { field } from "../ui/field";
import { icons } from "../ui/icons";
import { versionLine } from "../words";
import { contextColumn, iconLine, noticeSlot, refused, textSection, type NoticeSlot } from "./parts";

export type InviteMode = "invite" | "address";

const LABEL: Record<InviteMode, string> = { invite: "Invite link or code", address: "Server address" };

const EMPTY: Record<InviteMode, string> = {
  invite: "Paste the invite your host sent you.",
  address: "Type the address your host gave you.",
};

export interface InviteForm {
  el: HTMLFormElement;
  setMode(mode: InviteMode): void;
}

/**
 * The field and Continue, with a text button beside them that `link` names for each mode. A
 * refusal shows under the field; `added` hears of a server the model took.
 */
export function inviteForm(
  ctx: Ctx,
  slot: NoticeSlot,
  link: (mode: InviteMode) => [label: string, go: () => void],
  added: (mode: InviteMode) => void = () => {},
): InviteForm {
  let mode: InviteMode = "invite";
  const input = h("input", { type: "text", autocomplete: "off", spellcheck: "false" });
  const box = field(LABEL.invite, input);
  const go = button.primary("Continue");
  go.type = "submit";
  const other = button.link("");
  const submit = async (): Promise<void> => {
    const text = input.value.trim();
    if (text === "") {
      box.setError(EMPTY[mode]);
      input.focus();
      return;
    }
    box.setError(null);
    go.disabled = true;
    slot.hold(true);
    const refusal = await ctx.send({ type: "addServer", text });
    go.disabled = false;
    if (refusal === null) {
      slot.hold(false);
      added(mode);
      return;
    }
    refused(ctx, slot, refusal, (words) => box.setError(words));
    slot.hold(false);
    input.focus();
  };
  const el = h(
    "form",
    {
      class: "form form-invite",
      novalidate: true,
      onsubmit: (event) => {
        event.preventDefault();
        void submit();
      },
    },
    box.el,
    h("div", { class: "actions actions-form" }, go, other),
  );
  const setMode = (to: InviteMode): void => {
    mode = to;
    box.setLabel(LABEL[to]);
    box.setError(null);
    input.classList.toggle("mono", to === "address");
    const [label, onclick] = link(to);
    other.textContent = label;
    other.onclick = onclick;
  };
  setMode("invite");
  return { el, setMode };
}

const LEDE: Record<InviteMode, string> = {
  invite:
    "Play Swords of Legends Online on a private server that a friend hosts for your group. To start, paste the invite they sent you.",
  address: "Type the address of the server your friend hosts, as they gave it to you. Then sign in with your account.",
};

export function welcome(root: HTMLElement, ctx: Ctx): Screen {
  const art = blade({ kind: "unlit" });
  const slot = noticeSlot(ctx);
  const lede = h("p", { class: "prose lede" });
  const form = inviteForm(
    ctx,
    slot,
    (mode) =>
      mode === "invite"
        ? ["I already have an account", () => setMode("address")]
        : ["Have an invite? Create an account", () => setMode("invite")],
    // An invite shows Create your account by itself; an address goes on to Sign in.
    (mode) => {
      if (mode === "address") {
        ctx.setTab("account");
      }
    },
  );
  const setMode = (mode: InviteMode): void => {
    form.setMode(mode);
    lede.textContent = LEDE[mode];
  };
  const side = contextColumn();
  root.append(
    h(
      "main",
      { class: "main" },
      art,
      h(
        "div",
        { class: "hero hero-welcome" },
        h("h1", { class: "headline page-title" }, "Welcome to Fenji"),
        h("p", { class: "status-line" }, h("span", { class: "dot", "aria-hidden": "true" }), "No server yet"),
        lede,
        slot.el,
        form.el,
        h(
          "div",
          { class: "assurances" },
          iconLine(
            icons.box(16),
            "You need your own copy of Swords of Legends Online, from Steam or the Gameforge launcher.",
          ),
          iconLine(icons.lock(16), "Fenji never asks for your Steam or Gameforge password."),
        ),
      ),
    ),
    side.el,
  );
  setMode("invite");
  return {
    update(s) {
      art.update(bladeFor(s), groundFor(s));
      slot.update(s);
      side.show(
        "welcome",
        () => [
          cardSection("Server", h("p", { class: "column-text muted" }, "Not set. Paste the invite your host sent you.")),
          textSection(
            "About Fenji",
            "The official servers for Swords of Legends Online closed on 30 June 2023. With Fenji, a friend can host a private server for the people they invite.",
            "Nothing to buy, no payments, no tracking.",
          ),
        ],
        versionLine(s),
      );
    },
    destroy() {
      root.replaceChildren();
    },
  };
}

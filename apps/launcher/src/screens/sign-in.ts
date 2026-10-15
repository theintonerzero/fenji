// Sign in (launcher spec §3.2, §3.4; the SignIn board): the username, the password and "Remember
// this server", ticked. While Play waits for the sign-in (needSignIn) the screen is the same, with
// Cancel. "Have an invite? Create an account" brings back Welcome's invite field, or, inside
// Create your account, that form (`onInvite`).
import type { Ctx, Screen } from "../app";
import { h } from "../dom";
import { activeServer } from "../route";
import type { Snapshot } from "../types";
import { button } from "../ui/button";
import { checkbox, field } from "../ui/field";
import { icons } from "../ui/icons";
import { versionLine } from "../words";
import {
  contextColumn,
  formError,
  hostNoticeSection,
  iconLine,
  noticeSlot,
  refused,
  serverCard,
  USERNAME,
  USERNAME_RULE,
} from "./parts";
import { inviteForm } from "./welcome";

export interface SignInOptions {
  /** Where "Have an invite? Create an account" goes instead of the invite field. */
  onInvite?: () => void;
}

type Mode = "signIn" | "invite";

export function signIn(root: HTMLElement, ctx: Ctx, options: SignInOptions = {}): Screen {
  let mode: Mode = "signIn";
  let last: Snapshot | null = null;
  let filled = false;
  const slot = noticeSlot(ctx);
  const title = h("h1", { class: "headline page-title" });
  const lede = h("p", { class: "lede" });

  const user = h("input", { type: "text", autocomplete: "username", spellcheck: "false" });
  const pass = h("input", { type: "password", autocomplete: "current-password" });
  const userField = field("Username", user);
  const passField = field("Password", pass);
  const remember = checkbox("Remember this server", true);
  const problem = formError();
  const go = button.primary("Sign in");
  go.type = "submit";
  const cancel = button.secondary("Cancel", () => void ctx.send({ type: "cancelSignIn" }));
  const toInvite = button.link("Have an invite? Create an account", () =>
    options.onInvite ? options.onInvite() : setMode("invite"),
  );
  const submit = async (): Promise<void> => {
    const username = user.value.trim();
    const password = pass.value;
    const userError = USERNAME.test(username) ? null : USERNAME_RULE;
    const passError = password === "" ? "Type your password." : null;
    userField.setError(userError);
    passField.setError(passError);
    problem.set(null);
    if (userError !== null || passError !== null) {
      (userError !== null ? user : pass).focus();
      return;
    }
    go.disabled = true;
    slot.hold(true);
    const refusal = await ctx.send({ type: "signIn", username, password, remember: remember.input.checked });
    go.disabled = false;
    if (refusal === null) {
      pass.value = "";
    } else {
      refused(ctx, slot, refusal, (words) => problem.set(words));
    }
    slot.hold(false);
  };
  const form = h(
    "form",
    {
      class: "form form-narrow",
      novalidate: true,
      onsubmit: (event) => {
        event.preventDefault();
        void submit();
      },
    },
    h("div", { class: "fields" }, userField.el, passField.el),
    h("div", { class: "remember" }, remember.el),
    problem.el,
    h("div", { class: "actions actions-form" }, go, toInvite),
  );

  const invite = inviteForm(ctx, slot, () => ["I already have an account", () => setMode("signIn")]);
  const place = h("div", {}, form);
  const side = contextColumn();
  root.append(
    h(
      "main",
      { class: "main page" },
      title,
      lede,
      slot.el,
      place,
      iconLine(icons.lock(14), "Fenji never asks for your Steam or Gameforge password."),
    ),
    side.el,
  );

  function setMode(to: Mode): void {
    mode = to;
    place.replaceChildren(to === "signIn" ? form : invite.el);
    if (last !== null) {
      update(last);
    }
  }

  function update(s: Snapshot): void {
    last = s;
    const v = activeServer(s);
    const name = v?.name ?? "your server";
    if (mode === "signIn") {
      title.textContent = `Sign in to ${name}`;
      lede.textContent = "Use the account you made with your invite, not your Steam account.";
    } else {
      title.textContent = "Create your account";
      lede.textContent = "Paste the invite your host sent you.";
    }
    if (s.needSignIn && !cancel.isConnected) {
      go.after(cancel);
    } else if (!s.needSignIn) {
      cancel.remove();
    }
    if (!filled && v !== undefined) {
      filled = true;
      if (user.value === "" && v.username !== null) {
        user.value = v.username;
      }
    }
    slot.update(s);
    side.show(
      JSON.stringify(v ?? null),
      () => (v === undefined ? [] : [serverCard(v, ["status", "address", "host", "version"], Date.now()), hostNoticeSection(v)]),
      versionLine(s),
    );
  }

  return {
    update,
    destroy() {
      root.replaceChildren();
    },
  };
}

// Create your account (launcher spec §3.2, §3.4; the Redeem board): the invite the model holds,
// then a username and the password twice, checked here before they're sent. The server's taken-
// name sentence shows under Username, its other refusals under the form. "I already have an
// account" shows Sign in in its place: the invite waits until the player signs in, and the model
// drops it then.
import type { Ctx, Screen } from "../app";
import { h } from "../dom";
import { activeServer } from "../route";
import type { ServerView, Snapshot } from "../types";
import { button } from "../ui/button";
import { field } from "../ui/field";
import { icons } from "../ui/icons";
import { versionLine } from "../words";
import {
  contextColumn,
  formError,
  hostNoticeSection,
  iconLine,
  noticeSlot,
  PASSWORD_RULE,
  refused,
  serverCard,
  textSection,
  USERNAME,
  USERNAME_RULE,
} from "./parts";
import { signIn } from "./sign-in";

/** The server's own refusals that concern the username (app/words.rs: username_taken, bad_username). */
function aboutUsername(refusal: string): boolean {
  return refusal.includes(" is taken on ") || refusal === USERNAME_RULE;
}

/** Passwords count characters, as the server does, not UTF-16 units. */
function length(text: string): number {
  return [...text].length;
}

/** The server the invite is for: the active one, since adding an invite makes it active. */
function invited(s: Snapshot): ServerView | undefined {
  return s.servers.find((v) => v.id === s.pendingInvite?.serverId) ?? activeServer(s);
}

export function redeem(root: HTMLElement, ctx: Ctx): Screen {
  let last: Snapshot | null = null;
  let inner: Screen | null = null;
  const slot = noticeSlot(ctx);
  const lede = h("p", { class: "lede" });
  const invite = h("div", { class: "invite" });

  const user = h("input", { type: "text", autocomplete: "username", spellcheck: "false" });
  const pass = h("input", { type: "password", autocomplete: "new-password", "aria-describedby": "redeem-hint" });
  const again = h("input", { type: "password", autocomplete: "new-password" });
  const userField = field("Username", user);
  const passField = field("Password", pass);
  const againField = field("Confirm password", again);
  const problem = formError();
  const create = button.primary("Create account");
  create.type = "submit";
  const haveAccount = button.link("I already have an account", () => {
    void ctx.send({ type: "cancelSignIn" });
    root.replaceChildren();
    inner = signIn(root, ctx, { onInvite: back });
    if (last !== null) {
      inner.update(last);
    }
    ctx.setTab("account");
  });
  const submit = async (): Promise<void> => {
    const username = user.value.trim();
    const password = pass.value;
    const errors: [field: typeof userField, input: HTMLInputElement, text: string | null][] = [
      [userField, user, USERNAME.test(username) ? null : USERNAME_RULE],
      [passField, pass, length(password) >= 10 && length(password) <= 128 ? null : PASSWORD_RULE],
      [againField, again, again.value === password ? null : "The passwords don't match."],
    ];
    for (const [box, , text] of errors) {
      box.setError(text);
    }
    problem.set(null);
    const first = errors.find(([, , text]) => text !== null);
    if (first) {
      first[1].focus();
      return;
    }
    create.disabled = true;
    slot.hold(true);
    const refusal = await ctx.send({ type: "redeem", code: null, username, password });
    create.disabled = false;
    if (refusal === null) {
      pass.value = "";
      again.value = "";
    } else if (aboutUsername(refusal)) {
      refused(ctx, slot, refusal, (words) => userField.setError(words));
      user.focus();
    } else {
      refused(ctx, slot, refusal, (words) => problem.set(words));
    }
    slot.hold(false);
  };
  const form = h(
    "form",
    {
      class: "form form-wide",
      novalidate: true,
      onsubmit: (event) => {
        event.preventDefault();
        void submit();
      },
    },
    h("div", { class: "fields" }, userField.el, h("div", { class: "field-pair" }, passField.el, againField.el)),
    h("p", { class: "hint", id: "redeem-hint" }, "Pick one you don't use anywhere else, and never your Steam password."),
    problem.el,
    h("div", { class: "actions actions-form" }, create, haveAccount),
  );
  const foot = h("div", {});
  const side = contextColumn();
  const main = h(
    "main",
    { class: "main page" },
    h("h1", { class: "headline page-title" }, "Create your account"),
    lede,
    slot.el,
    invite,
    form,
    foot,
  );
  root.append(main, side.el);

  /** Back from Sign in to this form, as it was left. */
  function back(): void {
    inner?.destroy();
    inner = null;
    root.replaceChildren(main, side.el);
    if (last !== null) {
      update(last);
    }
  }

  function update(s: Snapshot): void {
    last = s;
    if (inner !== null) {
      inner.update(s);
      return;
    }
    const v = invited(s);
    const name = v?.name ?? "the server";
    lede.textContent = v?.host
      ? `${v.host} invited you to ${name}. Choose a username and a password.`
      : `You're invited to ${name}. Choose a username and a password.`;
    invite.replaceChildren(
      h("span", { class: "invite-tile", "aria-hidden": "true" }, icons.servers(18)),
      h(
        "div",
        { class: "invite-server" },
        h("div", { class: "invite-name" }, name),
        h("div", { class: "invite-address" }, v?.address ?? ""),
      ),
      h(
        "div",
        { class: "invite-code" },
        h("div", { class: "invite-ok" }, icons.check(13), "Invite recognised"),
        h("div", { class: "mono" }, s.pendingInvite?.code ?? ""),
      ),
    );
    foot.replaceChildren(
      iconLine(icons.lock(14), `Your password goes only to ${name}'s account service, which stores it only as a hash.`),
    );
    slot.update(s);
    side.show(
      JSON.stringify([v ?? null, s.client.folder === null]),
      () =>
        v === undefined
          ? []
          : [
              serverCard(v, ["status", "address", "host", "version"], Date.now()),
              hostNoticeSection(v),
              s.client.folder === null
                ? textSection(
                    "What's next",
                    "Show Fenji where your copy of Swords of Legends Online is, and it checks the files. Then you can play.",
                  )
                : null,
            ],
      versionLine(s),
    );
  }

  return {
    update,
    destroy() {
      inner?.destroy();
      root.replaceChildren();
    },
  };
}

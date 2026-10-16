// Join (the redesign's Create your account board): the invite the model holds for the active
// server, then a username and a password, checked here before they're sent. One password field,
// with Show to catch a typo: there's no reset, only a host deleting the account. The server's
// taken-name sentence shows under Username, its other refusals under the form. "I have an account"
// signs in instead; the model drops the invite then.
import type { Ctx, Screen } from "../app";
import { h } from "../dom";
import { activeServer } from "../route";
import { button } from "../ui/button";
import { field, revealToggle } from "../ui/field";
import { formError, noticeSlot, PASSWORD_RULE, refused, USERNAME, USERNAME_RULE } from "./parts";

/** The server's own refusals that concern the username (app/words.rs: username_taken, bad_username). */
function aboutUsername(refusal: string): boolean {
  return refusal.includes(" is taken on ") || refusal === USERNAME_RULE;
}

/** Passwords count characters, as the server does, not UTF-16 units. */
function length(text: string): number {
  return [...text].length;
}

export function redeem(root: HTMLElement, ctx: Ctx): Screen {
  const slot = noticeSlot(ctx);
  const title = h("h1", { class: "task-title" });
  const lede = h("p", { class: "lede" });
  const user = h("input", { type: "text", autocomplete: "username", spellcheck: "false" });
  const pass = h("input", { type: "password", autocomplete: "new-password", "aria-describedby": "redeem-hint" });
  const userField = field("Username", user);
  const passField = field("Password", pass, { trailing: revealToggle(pass) });
  const problem = formError();
  const create = button.primary("Create account");
  create.type = "submit";
  const signInstead = button.link("I have an account", () => ctx.signIn(true));
  const submit = async (): Promise<void> => {
    const username = user.value.trim();
    const password = pass.value;
    const userError = USERNAME.test(username) ? null : USERNAME_RULE;
    const passError = length(password) >= 10 && length(password) <= 128 ? null : PASSWORD_RULE;
    userField.setError(userError);
    passField.setError(passError);
    problem.set(null);
    if (userError !== null || passError !== null) {
      (userError !== null ? user : pass).focus();
      return;
    }
    create.disabled = true;
    slot.hold(true);
    const refusal = await ctx.send({ type: "redeem", code: null, username, password });
    create.disabled = false;
    if (refusal === null) {
      pass.value = "";
    } else if (aboutUsername(refusal)) {
      refused(ctx, slot, refusal, (words) => userField.setError(words));
      user.focus();
    } else {
      refused(ctx, slot, refusal, (words) => problem.set(words));
    }
    slot.hold(false);
  };
  root.append(
    h(
      "div",
      { class: "task" },
      title,
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
        h(
          "div",
          { class: "fields" },
          userField.el,
          h("div", {}, passField.el, h("p", { class: "hint", id: "redeem-hint" }, "Not your Steam password.")),
        ),
        problem.el,
        h("div", { class: "actions" }, create, signInstead),
      ),
    ),
  );
  let shownFor = "";
  return {
    update(s) {
      const v = activeServer(s);
      const name = v?.name ?? "the server";
      const key = JSON.stringify([name, v?.host ?? null]);
      if (key !== shownFor) {
        shownFor = key;
        title.textContent = `Join ${name}`;
        lede.replaceChildren(...(v?.host ? ["Hosted by ", h("span", { class: "gold" }, v.host), "."] : ["Choose a username and a password."]));
      }
      slot.update(s);
    },
    destroy() {
      root.replaceChildren();
    },
  };
}

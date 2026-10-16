// Sign in (the redesign's Sign in board): to the active server, when Play needs it (needSignIn,
// and Play carries on once signed in) or when the player asked (the Account popover, or "I have an
// account"). The username, the password with Show, and "Remember this server", ticked. Cancel goes
// back to where the player was.
import type { Ctx, Screen } from "../app";
import { h } from "../dom";
import { activeServer } from "../route";
import type { Snapshot } from "../types";
import { button } from "../ui/button";
import { checkbox, field, revealToggle } from "../ui/field";
import { formError, noticeSlot, refused, USERNAME, USERNAME_RULE } from "./parts";

export function signIn(root: HTMLElement, ctx: Ctx): Screen {
  let last: Snapshot | null = null;
  let filled = false;
  const slot = noticeSlot(ctx);
  const title = h("h1", { class: "task-title" });
  const lede = h("p", { class: "lede" });
  const user = h("input", { type: "text", autocomplete: "username", spellcheck: "false" });
  const pass = h("input", { type: "password", autocomplete: "current-password" });
  const userField = field("Username", user);
  const passField = field("Password", pass, { trailing: revealToggle(pass) });
  const remember = checkbox("Remember this server", true);
  const problem = formError();
  const go = button.primary("Sign in");
  go.type = "submit";
  const cancel = button.quiet("Cancel", () => {
    if (last?.needSignIn) {
      void ctx.send({ type: "cancelSignIn" });
    }
    ctx.signIn(null);
  });
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
      ctx.signIn(null);
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
        h("div", { class: "fields" }, userField.el, passField.el),
        h("div", { class: "remember" }, remember.el),
        problem.el,
        h("div", { class: "actions" }, go, cancel),
      ),
    ),
  );
  return {
    update(s) {
      last = s;
      const v = activeServer(s);
      const name = v?.name ?? "your server";
      if (title.textContent !== `Sign in to ${name}`) {
        title.textContent = `Sign in to ${name}`;
        lede.textContent = `Your ${name} account, not Steam.`;
      }
      if (!filled && v !== undefined) {
        filled = true;
        if (user.value === "" && v.username !== null) {
          user.value = v.username;
        }
      }
      slot.update(s);
    },
    destroy() {
      root.replaceChildren();
    },
  };
}

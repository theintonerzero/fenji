// The Account popover (the redesign's Account board): who the player is on each server, signed in
// or not, with Sign out on the active server and Sign in, or Create account for an invite, on the
// others. The host deletes accounts.
import type { Ctx, PopoverContent } from "../app";
import { h } from "../dom";
import type { ServerView, Snapshot } from "../types";
import { button } from "../ui/button";
import { renew } from "../screens/parts";

function invited(v: ServerView, s: Snapshot): boolean {
  return s.pendingInvite !== null && s.pendingInvite.serverId === v.id;
}

export function accountPopover(ctx: Ctx): PopoverContent {
  const content = h("div", { class: "sections" });
  /** Goes to `v` and its sign-in, or its invite. */
  const go = async (v: ServerView, signIn: boolean): Promise<void> => {
    ctx.open(null);
    await ctx.send({ type: "selectServer", id: v.id });
    if (signIn) {
      ctx.signIn(true);
    }
  };
  return {
    title: "Account",
    body: content,
    update(s) {
      const key = JSON.stringify([
        s.active,
        s.pendingInvite,
        s.servers.map((v) => [v.id, v.name, v.signedIn, v.username]),
      ]);
      renew(content, key, () => [
        ...s.servers.map((v) =>
          h(
            "section",
            { class: "sec" },
            h("p", { class: "big" }, v.name),
            v.signedIn
              ? h("p", { class: "line" }, "Signed in as ", h("span", { class: "gold" }, v.username ?? ""))
              : invited(v, s)
                ? h("p", { class: "line" }, "Invited · ", button.link("Create account", () => void go(v, false)))
                : h("p", { class: "line" }, "Not signed in · ", button.link("Sign in", () => void go(v, true))),
            v.signedIn && v.id === s.active
              ? h("div", { class: "acts" }, button.secondary("Sign out", () => void ctx.send({ type: "signOut" }), true))
              : null,
          ),
        ),
        h("section", { class: "sec" }, h("p", { class: "line faint" }, "To delete an account, ask the host.")),
      ]);
    },
  };
}

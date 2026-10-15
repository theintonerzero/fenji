// Your account (launcher spec §3.2, §3.4; the AccountTab board): who's signed in where, and Sign
// out, which ends the session on the server too. The host deletes accounts; Fenji's privacy page
// says so, and opens outside the launcher.
import type { Ctx, Screen } from "../app";
import { h } from "../dom";
import { activeServer } from "../route";
import type { Snapshot } from "../types";
import { button } from "../ui/button";
import { icons } from "../ui/icons";
import { avatar, people } from "../ui/people";
import { versionLine } from "../words";
import { contextColumn, noticeSlot, renew, serverCard } from "./parts";

/** PRIVACY.md as released: the main branch holds only tagged releases. */
export const PRIVACY = "https://github.com/theintonerzero/fenji/blob/main/PRIVACY.md";

export function account(root: HTMLElement, ctx: Ctx): Screen {
  const slot = noticeSlot(ctx);
  const lede = h("p", { class: "lede" });
  const who = h("div", { class: "account-who" });
  const signOut = button.secondary([icons.signOut(16), "Sign out"], () => void ctx.send({ type: "signOut" }), "compact");
  const words = h("span", {});
  const privacy = h(
    "button",
    { class: "inline-link", type: "button", onclick: () => void ctx.send({ type: "openUrl", url: PRIVACY }) },
    "privacy page",
  );
  const side = contextColumn();
  root.append(
    h(
      "main",
      { class: "main page" },
      h("h1", { class: "headline page-title" }, "Your account"),
      lede,
      slot.el,
      h("section", { class: "card account-card", "aria-label": "Account details" }, who, signOut),
      h(
        "p",
        { class: "account-note" },
        icons.info(16),
        h("span", {}, words, privacy, " explains."),
      ),
    ),
    side.el,
  );
  return {
    update(s: Snapshot) {
      const v = activeServer(s);
      const name = v?.name ?? "your server";
      const username = v?.username ?? "";
      renew(lede, `${name}\n${username}`, () => [
        `Signed in to ${name} as `,
        h("strong", { class: "person-gold" }, username),
        ".",
      ]);
      renew(who, JSON.stringify([name, username, v?.address]), () => [
        avatar(username, null, "large"),
        h(
          "div",
          { class: "account-text" },
          h("div", { class: "account-name" }, username),
          h("div", { class: "account-line" }, `on ${name} · ${v?.address ?? ""}`),
        ),
      ]);
      words.textContent = `Want your account deleted? ${
        v?.host ? `Ask ${v.host}, who hosts ${name}.` : `Ask whoever hosts ${name}.`
      } The host deletes accounts, as Fenji's `;
      slot.update(s);
      side.show(
        JSON.stringify([v ?? null, s.lobby]),
        () =>
          v === undefined
            ? []
            : [
                people(v.signedIn ? s.lobby : null, v.username, v.host),
                serverCard(v, ["address", "version", "players"], Date.now()),
              ],
        versionLine(s),
      );
    },
    destroy() {
      root.replaceChildren();
    },
  };
}

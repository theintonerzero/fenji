// Your copy is ready (launcher spec §3.5): the folder, the build it matches and when it was last
// checked, with "Check every file" and "Change folder". No board draws it; it stands in the hero
// like its siblings, and has no ember action: there is nothing the copy needs.
import type { Ctx, Screen } from "../app";
import { h } from "../dom";
import { bladeFor, groundFor } from "../route";
import { blade } from "../ui/blade";
import { button } from "../ui/button";
import { icons } from "../ui/icons";
import { clientLine, seen } from "../words";
import { clientColumn, path } from "./client-parts";
import { noticeSlot, renew } from "./parts";

export function clientReady(root: HTMLElement, ctx: Ctx): Screen {
  const art = blade({ kind: "unlit" });
  const folder = h("p", { class: "lede" });
  const slot = noticeSlot(ctx);
  const lines = h("div", { class: "play-meta" });
  const side = clientColumn();
  root.append(
    h(
      "main",
      { class: "main" },
      art,
      h(
        "div",
        { class: "hero hero-wide" },
        h("h1", { class: "headline" }, "Your copy is ready"),
        folder,
        slot.el,
        h(
          "div",
          { class: "actions" },
          button.secondary("Check every file", () => void ctx.send({ type: "check", level: "full" })),
          button.secondary("Change folder", () => void ctx.send({ type: "pickFolder", purpose: "game" })),
        ),
        lines,
      ),
    ),
    side.el,
  );
  return {
    update(s) {
      art.update(bladeFor(s), groundFor(s));
      renew(folder, `${s.client.folder}\n${s.platform.home}`, () => [path(s.client.folder ?? "", s.platform.home)]);
      const check = s.client.check;
      const when =
        check.kind === "verified"
          ? `${check.level === "full" ? "Every file checked" : "Quick check"} ${seen(check.atMs, Date.now())}`
          : null;
      renew(lines, `${clientLine(s, true)}\n${when}`, () => {
        const ok = icons.check(13);
        ok.classList.add("icon-ok");
        return [
          h("p", { class: "meta" }, ok, clientLine(s, true)),
          when === null ? null : h("p", { class: "meta" }, icons.clock(14), when),
          // A ready copy leads on to Play, in gold: the screen's buttons stay outlines.
          h("p", { class: "meta" }, button.link(["Go to Play", icons.chevron(14)], () => ctx.setTab("play"))),
        ];
      });
      slot.update(s);
      side.update(s);
    },
    destroy() {
      root.replaceChildren();
    },
  };
}

// Set up the game (launcher spec §3.5, §3.6; the ClientSetup board): the player's own copy, from
// a folder they have or a download from their Steam library. On Linux the download is the one
// ember action (the brief; the board drew it on Choose folder); elsewhere the Steam card says why
// it can't, and Choose folder is. Without a terminal Fenji can open, the exact command opens in a
// dialog, to run in the player's own.
import type { Ctx, Screen } from "../app";
import { h, type Child } from "../dom";
import { bladeFor, groundFor } from "../route";
import type { Snapshot } from "../types";
import { blade } from "../ui/blade";
import { button } from "../ui/button";
import type { Dialog } from "../ui/dialog";
import { icons } from "../ui/icons";
import { clientColumn, commandDialog, GAME, pathEnd } from "./client-parts";
import { iconLine, noticeSlot, renew } from "./parts";

function choice(icon: Element, title: string, ...body: Child[]): HTMLElement {
  return h(
    "section",
    { class: "card choice" },
    h("div", { class: "choice-head" }, h("span", { class: "choice-tile", "aria-hidden": "true" }, icon), h("h2", { class: "choice-title" }, title)),
    ...body,
  );
}


export function clientSetup(root: HTMLElement, ctx: Ctx): Screen {
  const art = blade({ kind: "unlit" });
  const slot = noticeSlot(ctx);
  const own = h("div", {});
  const steam = h("div", {});
  const manual = h("div", {});
  let open: Dialog | null = null;
  const side = clientColumn();
  root.append(
    h(
      "main",
      { class: "main" },
      art,
      h(
        "div",
        { class: "hero hero-wide hero-setup" },
        h("h1", { class: "headline" }, "Set up the game"),
        h("p", { class: "lede" }, "Fenji needs your own copy of the game. Choose how to get it."),
        slot.el,
        h("div", { class: "choices" }, own, steam),
        manual,
      ),
    ),
    side.el,
  );

  function update(s: Snapshot): void {
    art.update(bladeFor(s), groundFor(s));
    slot.update(s);
    const linux = s.platform.canDownload;
    const build = s.client.build;
    const found = s.client.steam;
    renew(own, JSON.stringify([linux, build, found, s.platform.home]), () => {
      const pick = (): void => void ctx.send({ type: "pickFolder", purpose: "game" });
      // A copy Steam installed leads, used where Steam keeps it (Amendment A, LA1).
      if (found !== null && found.pinned) {
        return [
          choice(
            icons.folder(18),
            "Use my game folder",
            h("p", { class: "choice-text" }, "Found in your Steam library:"),
            h("p", { class: "choice-text" }, pathEnd(found.folder, 64, s.platform.home)),
            iconLine(icons.circleCheck(16), `Build ${build}, the one Fenji plays. Fenji checks it and leaves it where Steam keeps it.`),
            h(
              "div",
              { class: "choice-foot" },
              button.primary("Use it", () => void ctx.send({ type: "chooseFolder", path: found.folder })),
              button.secondary("Choose another folder", pick),
            ),
          ),
        ];
      }
      // The ClientSetup board's emphasis: Choose folder is the ember everywhere, since most players
      // a host invites have the game installed already (the finish review's fix 2).
      return [
        choice(
          icons.folder(18),
          "Use my game folder",
          h("p", { class: "choice-text" }, "Already installed the game from Steam or the Gameforge launcher? Choose its folder."),
          found === null
            ? null
            : iconLine(icons.info(16), `Steam has build ${found.build} of the game. Fenji plays build ${build}: update the game in Steam, then come back.`),
          iconLine(icons.circleCheck(16), `Fenji checks it against build ${build} and points out any file that needs repair.`),
          h("div", { class: "choice-foot" }, button.primary("Choose folder", pick)),
        ),
      ];
    });
    renew(steam, `${linux}\n${build}\n${s.client.defaultFolder}`, () => [
      choice(
        icons.download(18),
        "Download with Steam",
        h(
          "p",
          { class: "choice-text" },
          `Needs ${GAME} in your Steam library. The launcher runs DepotDownloader to fetch only build ${build}.`,
        ),
        iconLine(icons.lock(16), "You sign in to Steam in DepotDownloader's own window, never in Fenji."),
        linux
          ? [
              h(
                "div",
                { class: "choice-folder" },
                icons.folder(16),
                pathEnd(s.client.defaultFolder, 18, s.platform.home),
                h(
                  "button",
                  {
                    class: "inline-link choice-change",
                    type: "button",
                    "aria-label": "Change download folder",
                    onclick: () => void ctx.send({ type: "pickFolder", purpose: "download" }),
                  },
                  "Change",
                ),
              ),
              h(
                "div",
                { class: "choice-foot" },
                button.secondary("Start download", () => void ctx.send({ type: "download", folder: s.client.defaultFolder })),
              ),
            ]
          : h("p", { class: "choice-text choice-why" }, "Downloading with Steam works on Linux only for now."),
      ),
    ]);
    const commands = s.client.manual;
    renew(manual, JSON.stringify(commands), () =>
      commands === null
        ? []
        : [
            h(
              "p",
              { class: "manual" },
              button.link("No Steam app? Run DepotDownloader yourself", () => {
                open?.close();
                open = commandDialog(ctx, commands.download, commands.get, "chooseFolder", () => {
                  open = null;
                });
                document.body.append(open.el);
              }),
            ),
          ],
    );
    side.update(s);
  }

  return {
    update,
    destroy() {
      open?.close();
      root.replaceChildren();
    },
  };
}

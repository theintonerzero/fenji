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
import { dialog, type Dialog } from "../ui/dialog";
import { icons } from "../ui/icons";
import { clientColumn, GAME, pathEnd } from "./client-parts";
import { iconLine, noticeSlot, renew } from "./parts";

function choice(icon: Element, title: string, ...body: Child[]): HTMLElement {
  return h(
    "section",
    { class: "card choice" },
    h("div", { class: "choice-head" }, h("span", { class: "choice-tile", "aria-hidden": "true" }, icon), h("h2", { class: "choice-title" }, title)),
    ...body,
  );
}

/** The command, selected, for a player to copy by hand when the clipboard isn't there. */
function select(el: Element): void {
  const range = document.createRange();
  range.selectNodeContents(el);
  const selection = window.getSelection();
  selection?.removeAllRanges();
  selection?.addRange(range);
}

/**
 * DepotDownloader's command for the player's own terminal (spec §3.6): their folder and the pinned
 * build filled in, their own Steam login left for them to type. Then they choose the folder.
 */
function commandDialog(ctx: Ctx, command: string, done: () => void): Dialog {
  const code = h("code", {}, command);
  const copy = button.secondary(
    "Copy",
    () => {
      const write = navigator.clipboard?.writeText(command);
      if (write === undefined) {
        select(code);
      } else {
        write.then(
          () => {
            copy.textContent = "Copied";
          },
          () => select(code),
        );
      }
    },
    "small",
  );
  const close = (): void => {
    shown.close();
    done();
  };
  const shown = dialog({
    title: "Run DepotDownloader yourself",
    icon: icons.download(20),
    description: "Run this in your own terminal, typing your Steam login where it asks. Then choose the folder it downloaded to.",
    body: [h("div", { class: "command-line" }, code, copy)],
    actions: [
      button.secondary("Close", close),
      button.primary("Choose folder", () => {
        void ctx.send({ type: "pickFolder", purpose: "game" });
        close();
      }),
    ],
    onClose: close,
  });
  return { el: shown.el, close };
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
    renew(own, `${linux}\n${build}`, () => {
      // The ClientSetup board's emphasis: Choose folder is the ember everywhere, since most players
      // a host invites have the game installed already (the finish review's fix 2).
      const choose = button.primary("Choose folder", () => void ctx.send({ type: "pickFolder", purpose: "game" }));
      return [
        choice(
          icons.folder(18),
          "Use my game folder",
          h("p", { class: "choice-text" }, "Already installed the game from Steam or the Gameforge launcher? Choose its folder."),
          iconLine(icons.circleCheck(16), `Fenji checks it against build ${build} and points out any file that needs repair.`),
          h("div", { class: "choice-foot" }, choose),
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
                pathEnd(s.client.defaultFolder, 18),
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
    const command = s.client.manualCommand;
    renew(manual, command ?? "", () =>
      command === null
        ? []
        : [
            h(
              "p",
              { class: "manual" },
              button.link("No Steam app? Run DepotDownloader yourself", () => {
                open?.close();
                open = commandDialog(ctx, command, () => {
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

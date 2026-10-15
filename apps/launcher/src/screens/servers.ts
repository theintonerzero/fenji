// Servers (launcher spec §3.3; the Servers and AddServer boards): every saved server in the order
// it was added, the active one marked. A click selects a row (SelectServer); the selected row's
// compact PLAY is the page's one ember action (PlayOn). "Add server" opens the Add dialog; the
// column has the selected server's facts, its source code, and "Remove from list" with a
// confirmation. A row's name is its button, so the list works from the keyboard too.
import type { Ctx, Screen } from "../app";
import { h } from "../dom";
import { activeServer } from "../route";
import type { ServerView, Snapshot } from "../types";
import { button } from "../ui/button";
import { facts } from "../ui/card";
import { dialog, type Dialog } from "../ui/dialog";
import { field } from "../ui/field";
import { icons } from "../ui/icons";
import { people } from "../ui/people";
import { statusMark } from "../ui/status";
import { serverLine, versionLine } from "../words";
import { contextColumn, noticeSlot, refused, renew, type NoticeSlot } from "./parts";

/** Whether the row's PLAY can start the game there now. */
function playable(v: ServerView, s: Snapshot): boolean {
  return v.status.kind === "online" && s.platform.canPlay && s.game.kind !== "starting" && s.game.kind !== "running";
}

function row(v: ServerView, s: Snapshot, ctx: Ctx, now: number): HTMLElement {
  const selected = v.id === s.active;
  // In maintenance the host's message says when it's back (the Servers board), as on Play.
  const notice = v.status.kind === "maintenance" && v.status.message !== "" ? v.status.message : v.motd;
  const address = v.pinned
    ? h("span", { class: "server-address" }, h("span", { class: "num" }, v.address), " · ", icons.lock(12), "own key")
    : h("span", { class: "server-address" }, v.address);
  const play = selected && playable(v, s)
    ? h(
        "button",
        {
          class: "btn btn-play btn-play-compact ember",
          type: "button",
          onclick: (event) => {
            event.stopPropagation();
            void ctx.send({ type: "playOn", id: v.id });
          },
        },
        "Play",
      )
    : null;
  return h(
    "li",
    {
      class: selected ? "server-row server-row-selected" : "server-row",
      "data-server": String(v.id),
      onclick: () => void ctx.send({ type: "selectServer", id: v.id }),
    },
    h("span", { class: "server-tile", "aria-hidden": "true" }, [...v.name][0]?.toUpperCase() ?? "?"),
    h(
      "div",
      { class: "server-main" },
      h(
        "div",
        { class: "server-head" },
        h("button", { class: "server-name", type: "button", "aria-pressed": selected ? "true" : "false" }, v.name),
        address,
      ),
      h("p", { class: "server-status" }, statusMark(v, true), h("span", {}, serverLine(v, s, now))),
      notice === ""
        ? null
        : h("p", { class: "server-motd notice" }, v.host ? [h("strong", {}, `${v.host}:`), " "] : null, notice),
    ),
    play,
  );
}

/** Add server (the AddServer board): an address or an invite link, its refusal under the field. Play
 * opens it too, for a server whose link must be added again. */
export function addDialog(ctx: Ctx, slot: NoticeSlot, done: () => void): Dialog {
  let refusedHere = false;
  const input = h("input", { type: "text", class: "mono", autocomplete: "off", spellcheck: "false" });
  const box = field("Address or invite link", input);
  const add = button.primary("Add server");
  add.type = "submit";
  add.setAttribute("form", "add-server");
  const close = (): void => {
    shown.close();
    done();
    // A refusal waited as the notice as well as under the field: it goes with the dialog.
    if (refusedHere) {
      void ctx.send({ type: "dismissNotice" });
    }
  };
  const submit = async (): Promise<void> => {
    const text = input.value.trim();
    if (text === "") {
      box.setError("Paste the link your host sent, or type the server's address.");
      input.focus();
      return;
    }
    add.disabled = true;
    slot.hold(true);
    const refusal = await ctx.send({ type: "addServer", text });
    add.disabled = false;
    if (refusal === null) {
      slot.hold(false);
      refusedHere = false;
      close();
      return;
    }
    refusedHere = true;
    slot.inline(refusal);
    box.setError(refusal);
    slot.hold(false);
    input.focus();
  };
  const shown = dialog({
    title: "Add a server",
    icon: icons.servers(20),
    description: "Paste the link your host sent you, or a server's address, such as play.example.net.",
    body: [
      h(
        "form",
        {
          id: "add-server",
          novalidate: true,
          onsubmit: (event) => {
            event.preventDefault();
            void submit();
          },
        },
        box.el,
      ),
    ],
    actions: [button.secondary("Cancel", close), add],
    onClose: close,
  });
  return { el: shown.el, close };
}

/** "Remove Moonward from your list? Your account on it stays." */
function removeDialog(ctx: Ctx, v: ServerView, done: () => void): Dialog {
  const close = (): void => {
    shown.close();
    done();
  };
  const shown = dialog({
    title: "Remove a server",
    icon: icons.trash(20),
    description: `Remove ${v.name} from your list? Your account on it stays.`,
    actions: [
      button.secondary("Cancel", close),
      button.primary("Remove", () => {
        void ctx.send({ type: "removeServer", id: v.id });
        close();
      }),
    ],
    onClose: close,
  });
  return { el: shown.el, close };
}

export function servers(root: HTMLElement, ctx: Ctx): Screen {
  let open: Dialog | null = null;
  const show = (make: (done: () => void) => Dialog): void => {
    open?.close();
    open = make(() => {
      open = null;
    });
    document.body.append(open.el);
  };
  const slot = noticeSlot(ctx);
  const list = h("ul", { class: "server-list", "aria-label": "Your servers" });
  const refresh = button.link([icons.refresh(16), "Refresh"], () => void ctx.send({ type: "refresh" }));
  const add = button.secondary([icons.plus(16), "Add server"], () => show((done) => addDialog(ctx, slot, done)), "compact");
  add.setAttribute("data-action", "add");
  const side = contextColumn();
  root.append(
    h(
      "main",
      { class: "main page" },
      h(
        "div",
        { class: "title-row" },
        h(
          "div",
          {},
          h("h1", { class: "headline page-title" }, "Servers"),
          h("p", { class: "lede" }, "The server you choose is the one Play opens."),
        ),
        h("div", { class: "title-actions" }, refresh, add),
      ),
      slot.el,
      list,
    ),
    side.el,
  );
  return {
    update(s) {
      const now = Date.now();
      renew(
        list,
        JSON.stringify([s.servers, s.active, s.lobby, s.platform.canPlay, s.game.kind, Math.floor(now / 60000)]),
        () => s.servers.map((v) => row(v, s, ctx, now)),
      );
      slot.update(s);
      const v = activeServer(s);
      side.show(
        JSON.stringify([v ?? null, s.lobby]),
        () => {
          if (v === undefined) {
            return [];
          }
          const host = v.host === null ? null : h("span", { class: "person-gold" }, v.host);
          return [
            h(
              "section",
              {},
              h("h2", { class: "column-heading" }, v.name),
              h(
                "div",
                { class: "card column-facts" },
                facts([
                  ["Address", v.address],
                  ["Version", v.version === null ? "Not known yet" : `Fenji ${v.version}`],
                  ["Players", v.players === null ? "Not known yet" : `${v.players.online} of ${v.players.max}`],
                  ["Ping", v.pingMs === null ? "No answer" : h("span", { class: "num" }, `${v.pingMs} ms`)],
                  ...(host === null ? [] : [["Hosted by", host] as [string, HTMLElement]]),
                ]),
                v.source === null
                  ? null
                  : button.link(["Source code", icons.external(13)], () => void ctx.send({ type: "openUrl", url: v.source! })),
              ),
            ),
            people(v.signedIn ? s.lobby : null, v.username, v.host),
            h(
              "div",
              { class: "column-end" },
              h(
                "button",
                {
                  class: "remove",
                  type: "button",
                  onclick: () => show((done) => removeDialog(ctx, v, done)),
                },
                icons.trash(15),
                `Remove ${v.name} from the list`,
              ),
            ),
          ];
        },
        versionLine(s),
      );
    },
    destroy() {
      open?.close();
      root.replaceChildren();
    },
  };
}

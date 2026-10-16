// The rail (R2): Fenji's seal, then each saved server as a square ink seal with its state in a 3 px
// bar beneath: jade online, amber in maintenance, faint offline or not yet known, cinnabar where this
// launcher can't play. The active seal is ember-edged, and + adds a server. A seal's tip shows its
// name and players on hover or focus. The arrow keys move along the rail. A seal's menu (a right
// click, the context-menu key or Shift+F10) has the server's source code and Remove, and Delete asks
// to remove it.
import type { Ctx } from "../app";
import { h } from "../dom";
import type { ServerView, Snapshot } from "../types";
import { sealLine } from "../words";
import { icons } from "./icons";

export type Bar = "ok" | "warn" | "off" | "bad";

export function barFor(v: ServerView): Bar {
  switch (v.status.kind) {
    case "online":
      return "ok";
    case "maintenance":
      return "warn";
    case "keyChanged":
    case "ownKey":
    case "incompatible":
      return "bad";
    default:
      return "off";
  }
}

/** A seal's letter: the name's first character, as a capital. */
export function sealLetter(name: string): string {
  return ([...name.trim()][0] ?? "?").toLocaleUpperCase();
}

export interface Rail {
  el: HTMLElement;
  /** `adding`: the Add a server dialog is open, and + shows it. */
  update(s: Snapshot, adding: boolean): void;
}

export function rail(ctx: Ctx): Rail {
  const list = h("ul", { class: "seals" });
  const add = h(
    "button",
    { class: "seal seal-add", type: "button", "aria-label": "Add a server", onclick: () => ctx.addServer() },
    icons.plus(20),
  );
  const tip = h("div", { class: "tip", "aria-hidden": "true", hidden: true });
  // Its empty space drags the window, as the top bar's does.
  const el = h(
    "nav",
    { class: "rail", "aria-label": "Servers", "data-tauri-drag-region": true },
    h("div", { class: "brand", "aria-hidden": "true", "data-tauri-drag-region": true }, "焚"),
    list,
    add,
    tip,
  );
  let shown = "";
  let menu: { el: HTMLElement; seal: HTMLElement } | null = null;

  const showTip = (seal: HTMLElement, v: ServerView): void => {
    tip.replaceChildren(h("strong", {}, v.name), h("span", {}, sealLine(v)));
    tip.style.top = `${seal.offsetTop + seal.offsetHeight / 2}px`;
    tip.hidden = false;
  };
  const hideTip = (): void => {
    tip.hidden = true;
  };

  const closeMenu = (refocus: boolean): void => {
    if (menu === null) {
      return;
    }
    const { el: open, seal } = menu;
    menu = null;
    open.remove();
    document.removeEventListener("mousedown", outside, true);
    if (refocus && seal.isConnected) {
      seal.focus();
    }
  };
  const outside = (event: MouseEvent): void => {
    if (menu !== null && !menu.el.contains(event.target as Node)) {
      closeMenu(false);
    }
  };
  const openMenu = (seal: HTMLElement, v: ServerView): void => {
    closeMenu(false);
    hideTip();
    const item = (words: string, act: () => void): HTMLButtonElement =>
      h(
        "button",
        {
          class: "menu-item",
          type: "button",
          role: "menuitem",
          tabindex: "-1",
          onclick: () => {
            closeMenu(true);
            act();
          },
        },
        words,
      );
    const source = v.source;
    const items = [
      source === null ? null : item("Source code", () => void ctx.send({ type: "openUrl", url: source })),
      item("Remove from list", () => ctx.removeServer(v)),
    ].filter((b): b is HTMLButtonElement => b !== null);
    const open = h("div", { class: "menu", role: "menu", "aria-label": v.name }, items);
    open.style.top = `${seal.offsetTop}px`;
    open.addEventListener("keydown", (event) => {
      const at = items.indexOf(document.activeElement as HTMLButtonElement);
      if (event.key === "Escape" || event.key === "Tab") {
        event.preventDefault();
        closeMenu(true);
      } else if (event.key === "ArrowDown" || event.key === "ArrowUp") {
        event.preventDefault();
        const step = event.key === "ArrowDown" ? 1 : items.length - 1;
        items[(at + step) % items.length]!.focus();
      }
    });
    el.append(open);
    menu = { el: open, seal };
    document.addEventListener("mousedown", outside, true);
    items[0]!.focus();
  };

  const seal = (v: ServerView, active: boolean): HTMLElement => {
    const b: HTMLButtonElement = h(
      "button",
      {
        class: active ? "seal on" : "seal",
        type: "button",
        "data-server": String(v.id),
        "data-bar": barFor(v),
        "aria-label": `${v.name}, ${sealLine(v)}`,
        "aria-current": active ? "true" : null,
        onclick: () => ctx.select(v.id),
        onmouseenter: () => showTip(b, v),
        onmouseleave: hideTip,
        onfocus: () => showTip(b, v),
        onblur: hideTip,
        oncontextmenu: (event) => {
          event.preventDefault();
          openMenu(b, v);
        },
        onkeydown: (event) => {
          const key = (event as KeyboardEvent).key;
          if (key === "ContextMenu" || (key === "F10" && (event as KeyboardEvent).shiftKey)) {
            event.preventDefault();
            openMenu(b, v);
          } else if (key === "Delete") {
            event.preventDefault();
            ctx.removeServer(v);
          }
        },
      },
      h("span", { class: "seal-letter", "aria-hidden": "true" }, sealLetter(v.name)),
    );
    return h("li", {}, b);
  };

  // Up and down the rail, + included.
  el.addEventListener("keydown", (event) => {
    if (menu !== null) {
      return;
    }
    const all = [...el.querySelectorAll<HTMLElement>("button.seal")];
    const at = all.indexOf(document.activeElement as HTMLElement);
    if (at < 0) {
      return;
    }
    const to =
      event.key === "ArrowDown"
        ? Math.min(at + 1, all.length - 1)
        : event.key === "ArrowUp"
          ? Math.max(at - 1, 0)
          : event.key === "Home"
            ? 0
            : event.key === "End"
              ? all.length - 1
              : null;
    if (to !== null) {
      event.preventDefault();
      all[to]!.focus();
    }
  });

  return {
    el,
    update(s, adding) {
      add.classList.toggle("open", adding);
      const key = JSON.stringify([s.active, s.servers.map((v) => [v.id, v.name, barFor(v), sealLine(v), v.source])]);
      if (key === shown) {
        return;
      }
      shown = key;
      // A probe draws the seals again every 15–30 s: the one that had focus keeps it.
      const focused = (document.activeElement as HTMLElement | null)?.closest?.("[data-server]")?.getAttribute("data-server");
      closeMenu(false);
      hideTip();
      list.replaceChildren(...s.servers.map((v) => seal(v, v.id === s.active)));
      if (focused !== null && focused !== undefined) {
        list.querySelector<HTMLElement>(`[data-server="${focused}"]`)?.focus();
      }
    },
  };
}

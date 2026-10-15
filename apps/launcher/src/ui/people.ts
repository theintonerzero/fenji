// Who's playing (DESIGN.md, Lists; plan LP5): 34 px avatars with initials, the name, and a short
// line, under "Playing now". The host and the player sit on ember-tint with gold initials; the
// others rotate through tinted grounds.
import { h } from "../dom";

/** "LY" for lin_yue, "W" for Wen. */
export function initials(name: string): string {
  const parts = name.split(/[\s_.-]+/).filter(Boolean);
  const letters = (parts.length > 1 ? parts.slice(0, 2).map((p) => p[0]) : [name[0] ?? "?"]).join("");
  return letters.toUpperCase();
}

export function avatar(name: string, tint: number | null, small = false): HTMLElement {
  return h(
    "span",
    {
      class: small ? "avatar avatar-small" : "avatar",
      "data-tint": tint === null ? null : String(tint),
      "aria-hidden": "true",
    },
    initials(name),
  );
}

/**
 * The "Playing now" section. `names` is who's online on the active server (`null` while signed
 * out there), `you` the player's own name, `host` the server's host.
 */
export function people(names: string[] | null, you: string | null, host: string | null): HTMLElement {
  const heading = h("h2", { class: "column-heading" }, "Playing now");
  if (names === null) {
    return h("section", {}, heading, h("p", { class: "people-empty" }, "Sign in to see who's online."));
  }
  if (names.length === 0) {
    return h("section", {}, heading, h("p", { class: "people-empty" }, "No one is playing right now."));
  }
  let others = 0;
  const rows = names.map((name) => {
    const own = name === you;
    const hosts = host !== null && name === host;
    const tint = own || hosts ? null : (others++ % 3) + 1;
    const line = own ? "You · online" : hosts ? "Host · online" : "Online";
    return h(
      "li",
      { class: "person" },
      avatar(name, tint),
      h("div", {}, h("div", { class: "person-name" }, name), h("div", { class: "person-line" }, line)),
    );
  });
  return h("section", {}, heading, h("ul", { class: "people" }, rows));
}

// Who's playing (R4): under PLAY, up to four faces with initials and the names in gold, "Kai, Wen,
// Shu playing". Running, the player leads as "You", on ember tint.
import { h } from "../dom";

/** "LY" for lin_yue, "W" for Wen. */
export function initials(name: string): string {
  const parts = name.split(/[\s_.-]+/).filter(Boolean);
  const letters = (parts.length > 1 ? parts.slice(0, 2).map((p) => [...p][0]) : [[...name][0] ?? "?"]).join("");
  return letters.toUpperCase();
}

const FACES = 4;
const NAMES = 3;

/**
 * The friends' row: `names` is who's online, the player left out; `you`, the player's own name
 * while their game runs. Nothing when no one is.
 */
export function friends(names: string[], you: string | null): HTMLElement | null {
  const everyone = you === null ? names : [you, ...names];
  if (everyone.length === 0) {
    return null;
  }
  const faces = everyone.slice(0, FACES).map((name, i) =>
    h("span", { class: you !== null && i === 0 ? "face face-you" : "face", "aria-hidden": "true" }, initials(name)),
  );
  // Three friends by name, and the player before them, as the Running board has it.
  const shown = [...(you === null ? [] : ["You"]), ...names.slice(0, NAMES)];
  const more = names.length - Math.min(names.length, NAMES);
  return h(
    "div",
    { class: "friends" },
    h("span", { class: "faces" }, faces),
    h("span", {}, h("span", { class: "gold" }, shown.join(", ")), more > 0 ? ` and ${more} more playing` : " playing"),
  );
}

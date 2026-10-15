// Cards (DESIGN.md, Cards / Containers): ink-850, a 1 px line, 12 px corners, no shadow
// (The Hairline Rule).
import { h, type Child } from "../dom";

export function card(title: string | null, ...children: Child[]): HTMLElement {
  return h(
    "section",
    { class: "card" },
    title === null ? null : h("h3", { class: "card-title" }, title),
    ...children,
  );
}

/** A definition list of facts: muted terms, plain values. */
export function facts(rows: [term: string, value: Child][]): HTMLDListElement {
  return h(
    "dl",
    { class: "facts" },
    rows.map(([term, value]) => [h("dt", {}, term), h("dd", {}, value)]),
  );
}

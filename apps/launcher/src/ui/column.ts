// The context column (DESIGN.md, The context column): ink-900, 340 px, sections 22 px apart with
// 14 px headings, and the launcher's version at its foot.
import { h, type Child } from "../dom";

export function column(sections: Child[], foot: string | null = null): HTMLElement {
  return h(
    "aside",
    { class: "column" },
    ...sections,
    foot === null ? null : h("p", { class: "column-foot" }, foot),
  );
}

/** A titled section of the column. */
export function section(heading: string, ...children: Child[]): HTMLElement {
  return h("section", {}, h("h2", { class: "column-heading" }, heading), ...children);
}

/** A section drawn as a card, as the boards draw the server's facts. */
export function cardSection(heading: string, ...children: Child[]): HTMLElement {
  return h("section", { class: "card column-card" }, h("h2", { class: "column-heading" }, heading), ...children);
}

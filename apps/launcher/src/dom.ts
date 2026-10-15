// Building elements without an HTML parser (launcher plan Task 12). Every string becomes a text
// node and every attribute a plain value, so a server's or a player's words never become markup.
// Nothing in src/ hands a string to the HTML parser: scripts/check-no-html.mjs (run by
// `pnpm run check`) refuses each of the DOM's ways to do it.

export type Child = Node | string | number | null | undefined | false | Child[];

/** Attributes by name; a function under `on…` is an event listener (`onclick` → `click`). */
export type Attrs = Record<
  string,
  string | number | boolean | null | undefined | ((event: Event) => void)
>;

function apply(el: Element, attrs: Attrs): void {
  for (const [name, value] of Object.entries(attrs)) {
    if (value === null || value === undefined || value === false) {
      continue;
    }
    if (typeof value === "function") {
      el.addEventListener(name.slice(2).toLowerCase(), value);
    } else {
      el.setAttribute(name, value === true ? "" : String(value));
    }
  }
}

function append(el: Element, children: Child[]): void {
  for (const child of children) {
    if (child === null || child === undefined || child === false) {
      continue;
    }
    if (Array.isArray(child)) {
      append(el, child);
    } else if (typeof child === "string" || typeof child === "number") {
      el.append(document.createTextNode(String(child)));
    } else {
      el.append(child);
    }
  }
}

/** An HTML element with its attributes and children. */
export function h<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  attrs: Attrs = {},
  ...children: Child[]
): HTMLElementTagNameMap[K] {
  const el = document.createElement(tag);
  apply(el, attrs);
  append(el, children);
  return el;
}

const SVG = "http://www.w3.org/2000/svg";

/** An SVG element, in SVG's namespace. */
export function s(tag: string, attrs: Attrs = {}, ...children: Child[]): SVGElement {
  const el = document.createElementNS(SVG, tag) as SVGElement;
  apply(el, attrs);
  append(el, children);
  return el;
}

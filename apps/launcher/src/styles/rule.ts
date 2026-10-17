// For the stylesheets' tests: what a rule declares.
import { expect } from "vitest";

/** The declarations of the rule whose selector list is `selector` alone: not one that ends a list,
 * as `html,\nbody` does. */
export function rule(css: string, selector: string): string {
  const escaped = selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const at = [...css.matchAll(new RegExp(`(^|\\n)${escaped} \\{`, "g"))].find(
    (m) => !/,\s*$/.test(css.slice(0, m.index)),
  )?.index;
  expect(at, selector).toBeDefined();
  const open = css.indexOf("{", at);
  return css.slice(open + 1, css.indexOf("}", open));
}

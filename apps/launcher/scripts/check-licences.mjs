#!/usr/bin/env node
// The launcher's npm production dependencies may carry only these licences (the launcher plan's
// Global Constraints). An SPDX expression passes when one side of each OR does and every side of
// each AND; an exception (WITH) never does. Run from apps/launcher after `pnpm install`.
import { execFileSync } from "node:child_process";

const ALLOWED = new Set(["MIT", "ISC", "BSD-2-Clause", "BSD-3-Clause", "Apache-2.0", "0BSD"]);

/** Whether an SPDX licence expression is allowed. */
export function allowed(expression) {
  const tokens = expression.match(/\(|\)|[^\s()]+/g) ?? [];
  let at = 0;
  const peek = () => tokens[at];
  const next = () => tokens[at++];
  function or() {
    let ok = and();
    while (peek() === "OR") {
      next();
      ok = and() || ok;
    }
    return ok;
  }
  function and() {
    let ok = one();
    while (peek() === "AND") {
      next();
      ok = one() && ok;
    }
    return ok;
  }
  function one() {
    const token = next();
    if (token === "(") {
      const ok = or();
      if (next() !== ")") throw new Error(`unbalanced: ${expression}`);
      return ok;
    }
    if (peek() === "WITH") {
      next();
      next();
      return false;
    }
    return token !== undefined && ALLOWED.has(token);
  }
  const ok = or();
  return ok && at === tokens.length;
}

if (import.meta.url === `file://${process.argv[1]}`) {
  const report = JSON.parse(
    execFileSync("pnpm", ["licenses", "list", "--prod", "--json"], { encoding: "utf8" }),
  );
  const refused = [];
  let count = 0;
  for (const [licence, packages] of Object.entries(report)) {
    for (const p of packages) {
      count += 1;
      if (!allowed(licence)) refused.push(`${p.name}@${p.versions.join(", ")}: ${licence}`);
    }
  }
  if (refused.length > 0) {
    console.error(`Licences the launcher doesn't allow in production:\n${refused.join("\n")}`);
    process.exit(1);
  }
  console.log(`${count} production package(s), each under an allowed licence`);
}

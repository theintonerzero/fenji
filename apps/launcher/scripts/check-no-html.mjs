#!/usr/bin/env node
// Fails when anything in src/ could hand a string to the HTML parser (launcher plan Task 12): the
// view builds every element with dom.ts's h(), so a server's or a player's words are only ever
// text. Run by `pnpm run check`, from apps/launcher.
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";

const PARSES_HTML = /\b(?:innerHTML|outerHTML|insertAdjacentHTML)\b|\bdocument\s*\.\s*write(?:ln)?\b/;

function* sources(dir) {
  for (const name of readdirSync(dir)) {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) {
      yield* sources(path);
    } else if (/\.(?:ts|js|mjs)$/.test(name)) {
      yield path;
    }
  }
}

const found = [];
for (const file of sources("src")) {
  readFileSync(file, "utf8")
    .split("\n")
    .forEach((line, i) => {
      if (PARSES_HTML.test(line)) {
        found.push(`${file}:${i + 1}: ${line.trim()}`);
      }
    });
}
if (found.length > 0) {
  console.error(`These hand a string to the HTML parser; build the elements with dom.ts's h():\n${found.join("\n")}`);
  process.exit(1);
}
console.log("Nothing in src/ hands a string to the HTML parser.");

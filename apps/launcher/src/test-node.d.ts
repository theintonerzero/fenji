// The Node APIs the view's tests use: they run in Node, the app never does. (@types/node isn't
// among the launcher's dev dependencies.)
declare module "node:fs" {
  export function readFileSync(path: string, encoding: "utf8"): string;
}

declare const process: { cwd(): string };

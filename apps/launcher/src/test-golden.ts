// The one loader every test uses: a golden snapshot by name, from fixtures/snapshots/ (launcher
// plan LP2). A fresh copy each time, so a test can change it.
import type { Snapshot } from "./types";

const files = import.meta.glob<Snapshot>("../fixtures/snapshots/*.json", {
  eager: true,
  import: "default",
});

export function golden(name: string): Snapshot {
  const snapshot = files[`../fixtures/snapshots/${name}.json`];
  if (!snapshot) {
    throw new Error(`no golden snapshot named ${name}`);
  }
  return structuredClone(snapshot);
}

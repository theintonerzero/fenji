// The view's side of the shell (launcher spec §3.1, plan LP1): one command for every intent, one
// for the current snapshot, and the `snapshot` event the shell sends on each change.
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

import type { Intent, Snapshot } from "./types";

export function snapshot(): Promise<Snapshot> {
  return invoke<Snapshot>("snapshot");
}

/** Calls `cb` with each new snapshot; the promise gives back the function that stops it. */
export function onSnapshot(cb: (s: Snapshot) => void): Promise<() => void> {
  return listen<Snapshot>("snapshot", (event) => cb(event.payload));
}

/** Sends an intent: `null` when it was done, otherwise the sentence that says why not. */
export async function send(intent: Intent): Promise<string | null> {
  try {
    await invoke("intent", { intent });
    return null;
  } catch (refusal) {
    return typeof refusal === "string" ? refusal : String(refusal);
  }
}

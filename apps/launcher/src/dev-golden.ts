// Development only (`pnpm dev`; the production build leaves this module out): `?golden=<name>`
// renders that golden snapshot, on `&tab=<tab>`, with a `send` that logs instead of acting. The
// finish review's screenshots use it (launcher plan Task 17).
import type { Ipc } from "./app";
import type { Tab } from "./route";
import type { Intent, Snapshot } from "./types";

const files = import.meta.glob<Snapshot>("../fixtures/snapshots/*.json", { import: "default" });
const TABS: Tab[] = ["play", "servers", "account", "client", "settings"];

export async function goldenIpc(params: URLSearchParams): Promise<{ ipc: Ipc; tab: Tab } | null> {
  const name = params.get("golden");
  if (name === null) {
    return null;
  }
  const load = files[`../fixtures/snapshots/${name}.json`];
  if (!load) {
    console.error(`No golden snapshot named ${name}.`);
    return null;
  }
  const snapshot = await load();
  const asked = params.get("tab") as Tab | null;
  return {
    tab: asked !== null && TABS.includes(asked) ? asked : "play",
    ipc: {
      snapshot: async () => snapshot,
      onSnapshot: async () => () => {},
      send: async (intent: Intent) => {
        console.info("intent", intent);
        return null;
      },
    },
  };
}

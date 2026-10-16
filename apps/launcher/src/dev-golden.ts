// Development only (`pnpm dev`; the production build leaves this module out): `?golden=<name>`
// renders that golden snapshot, with `&open=game|account|settings` for a popover, and a `send` that
// logs instead of acting. The finish review's screenshots use it.
import type { Ipc, MountOptions, PopoverName } from "./app";
import type { Intent, Snapshot } from "./types";

const files = import.meta.glob<Snapshot>("../fixtures/snapshots/*.json", { import: "default" });
const POPOVERS: PopoverName[] = ["game", "account", "settings"];

export async function goldenIpc(params: URLSearchParams): Promise<{ ipc: Ipc; options: MountOptions } | null> {
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
  const asked = params.get("open") as PopoverName | null;
  return {
    options: asked !== null && POPOVERS.includes(asked) ? { open: asked } : {},
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

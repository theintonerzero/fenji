// The launcher's view: the styles, then the frame over the shell (or, in development, over a
// golden snapshot named in the address).
import "./styles/tokens.css";
import "./styles/base.css";
import "./styles/components.css";

import { mount, type Ipc, type MountOptions } from "./app";
import * as live from "./ipc";

let ipc: Ipc = live;
let options: MountOptions = {};
if (import.meta.env.DEV) {
  const { goldenIpc } = await import("./dev-golden");
  const golden = await goldenIpc(new URLSearchParams(location.search));
  if (golden) {
    ({ ipc, options } = golden);
  }
}
await mount(document.body, ipc, options);

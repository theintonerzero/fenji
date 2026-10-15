import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { afterEach, expect, test } from "vitest";

import { send } from "./ipc";

afterEach(() => clearMocks());

test("a refused intent comes back as its sentence", async () => {
  mockIPC((cmd) => {
    if (cmd === "intent") {
      throw "Add a server first: paste the link or the address your host sent.";
    }
  });
  expect(await send({ type: "signOut" })).toBe(
    "Add a server first: paste the link or the address your host sent.",
  );
});

test("a done intent comes back as null, with the intent as the command's argument", async () => {
  let sent: unknown;
  mockIPC((cmd, args) => {
    if (cmd === "intent") {
      sent = args;
      return null;
    }
  });
  expect(await send({ type: "selectServer", id: 3 })).toBeNull();
  expect(sent).toEqual({ intent: { type: "selectServer", id: 3 } });
});

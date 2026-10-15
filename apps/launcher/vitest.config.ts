import { defineConfig } from "vitest/config";

// The view's tests: a DOM (jsdom), and UTC, so the words' times read the same on every machine.
export default defineConfig({
  test: {
    environment: "jsdom",
    env: { TZ: "UTC" },
  },
});

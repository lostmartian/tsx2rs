import { defineConfig } from "vitest/config";

/** The tsx2rs server against `next start` on a real build (slow; run with `pnpm test:runtime`). */
export default defineConfig({
  test: {
    name: "runtime-e2e",
    root: import.meta.dirname,
    include: ["test-e2e/**/*.test.ts"],
    testTimeout: 60_000,
    hookTimeout: 300_000,
  },
});

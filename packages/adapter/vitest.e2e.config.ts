import { defineConfig } from "vitest/config";

/** The adapter in real `next build`s (slow; run with `pnpm test:adapter`). */
export default defineConfig({
  test: {
    name: "adapter-e2e",
    root: import.meta.dirname,
    include: ["test-e2e/**/*.test.ts"],
    testTimeout: 300_000,
  },
});

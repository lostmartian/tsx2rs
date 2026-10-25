import { defineConfig } from "vitest/config";

/** The fixture tests: build and serve real Next.js apps (slow; run with `pnpm test:identity`). */
export default defineConfig({
  test: {
    name: "identity-e2e",
    root: import.meta.dirname,
    include: ["test-e2e/**/*.test.ts"],
    testTimeout: 60_000,
    hookTimeout: 300_000,
    // Fixtures build and run Next.js servers: one file at a time keeps CPU contention low.
    fileParallelism: false,
  },
});

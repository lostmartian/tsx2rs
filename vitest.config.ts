import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    projects: ["packages/*", "tests/identity"],
    coverage: {
      provider: "v8",
      include: ["packages/*/src/**", "tests/identity/src/**"],
      // Runs real Next.js processes: covered by `pnpm test:identity`, not the unit tests.
      exclude: ["tests/identity/src/next.ts"],
      // Floors from docs/engineering/testing.md
      thresholds: { lines: 80, functions: 80, branches: 80, statements: 80 },
    },
  },
});

import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    projects: ["packages/*", "tests/identity"],
    coverage: {
      provider: "v8",
      include: ["packages/*/src/**", "tests/identity/src/**"],
      // Floors from docs/engineering/testing.md
      thresholds: { lines: 80, functions: 80, branches: 80, statements: 80 },
    },
  },
});

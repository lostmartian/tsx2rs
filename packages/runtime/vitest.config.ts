import { defineProject } from "vitest/config";

export default defineProject({
  test: {
    name: "runtime",
    include: ["test/**/*.test.ts"],
  },
});

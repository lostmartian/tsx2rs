import { defineProject } from "vitest/config";

export default defineProject({
  test: {
    name: "adapter",
    include: ["test/**/*.test.ts"],
  },
});

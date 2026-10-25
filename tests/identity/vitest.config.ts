import { defineProject } from "vitest/config";

export default defineProject({
  test: {
    name: "identity",
    include: ["test/**/*.test.ts"],
  },
});

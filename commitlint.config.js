// Conventional Commits, as described in CONTRIBUTING.md.
import { RuleConfigSeverity } from "@commitlint/types";

/** @type {import("@commitlint/types").UserConfig} */
export default {
  extends: ["@commitlint/config-conventional"],
  rules: {
    "header-max-length": [RuleConfigSeverity.Error, "always", 72],
    "scope-enum": [
      RuleConfigSeverity.Error,
      "always",
      [
        "engine",
        "compiler",
        "ir",
        "napi",
        "adapter",
        "runtime",
        "native",
        "identity",
        "repo",
        "deps",
        "deps-dev",
        "bench",
      ],
    ],
  },
};

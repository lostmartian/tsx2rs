// Conventional Commits, as described in CONTRIBUTING.md.
import { RuleConfigSeverity } from "@commitlint/types";

/** @type {import("@commitlint/types").UserConfig} */
export default {
  extends: ["@commitlint/config-conventional"],
  // Dependabot's titles are machine-written ("build(deps-dev): bump x from 1.0.0 to 1.0.1 in the
  // npm-minor group") and often longer than the header limit.
  ignores: [(message) => /^(build|ci)\(deps(-dev)?\): bump /.test(message)],
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

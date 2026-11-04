// @ts-check
import eslint from "@eslint/js";
import { defineConfig } from "eslint/config";
import tseslint from "typescript-eslint";

export default defineConfig(
  {
    ignores: [
      "**/target/",
      "**/node_modules/",
      "**/coverage/",
      "**/dist/",
      "**/.next/",
      "**/next-env.d.ts",
      "packages/native/binding/",
      // Vendored third-party test apps
      "tests/apps/",
    ],
  },
  eslint.configs.recommended,
  tseslint.configs.strictTypeChecked,
  tseslint.configs.stylisticTypeChecked,
  {
    languageOptions: {
      parserOptions: {
        projectService: true,
        tsconfigRootDir: import.meta.dirname,
      },
    },
  },
  {
    // Component fixtures exercise JavaScript's own semantics (`||` beside `??`, values the
    // types say can't be nullish), which these rules would rewrite away.
    files: ["tests/differential/components/**"],
    rules: {
      "@typescript-eslint/prefer-nullish-coalescing": "off",
      "@typescript-eslint/no-unnecessary-condition": "off",
    },
  },
);

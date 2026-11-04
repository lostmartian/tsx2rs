/**
 * The bundler loader (ADR-0017): appends a server module's data functions and their
 * registration to its source, in Next.js's own server bundle. Added by the adapter for
 * Turbopack and webpack alike; client bundles never run it. A module the compiler didn't
 * compile passes through unchanged.
 */
import { readFileSync } from "node:fs";
import { relative } from "node:path";

interface Options {
  /** The project's root. */
  root: string;
  /** The compile result's file, written by the adapter's `modifyConfig`. */
  build: string;
}

interface LoaderContext {
  resourcePath: string;
  getOptions(): Options;
}

let cache: { file: string; modules: Record<string, string> } | undefined;

/** The appended code by module path, read once per process. */
function modules(file: string): Record<string, string> {
  if (cache?.file !== file) {
    const { modules } = JSON.parse(readFileSync(file, "utf8")) as {
      modules: Record<string, string>;
    };
    cache = { file, modules };
  }
  return cache.modules;
}

export default function loader(this: LoaderContext, source: string): string {
  const { root, build } = this.getOptions();
  const path = relative(root, this.resourcePath).replaceAll("\\", "/");
  const code = modules(build)[path];
  return code === undefined ? source : `${source}\n${code}`;
}

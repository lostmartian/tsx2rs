/**
 * The manifest (ADR-0017): what `next build` produced for the runtime. Versioned JSON in
 * `<distDir>/tsx2rs/manifest.json`, with one file per program and per encoder beside it.
 */
import { mkdirSync, writeFileSync } from "node:fs";
import { join, relative } from "node:path";
import type { App, DataFunction } from "@tsx2rs/native";
import { sourcePage } from "./pages.ts";

/** The manifest format's version: bumped on every incompatible change. */
export const MANIFEST_VERSION = 1;

/** A page route in the manifest. */
export interface ManifestRoute {
  /** The page file, relative to the project. */
  page: string;
  /** The pathnames Next.js serves it at (`/p/[n]`, and `/p/[n].rsc` for client navigations). */
  pathnames: string[];
  /** Next.js's entrypoint for it, relative to the project (the runtime's fallback, and where its
   * modules load from). */
  entrypoint: string | null;
  compiled: boolean;
  /** Why it falls back; empty when it compiled. */
  reasons: string[];
  /** The program file, relative to the manifest's folder. */
  program?: string;
  /** The generated encoder module, relative to the manifest's folder. */
  encoder?: string;
  /** The page's data function. */
  entry?: DataFunction;
  /** The data functions the encoder's `data` array holds, in order. */
  callees?: DataFunction[];
}

export interface Manifest {
  version: typeof MANIFEST_VERSION;
  nextVersion: string;
  buildId: string;
  routes: ManifestRoute[];
}

/** What Next.js says it built for each app page (`outputs.appPages`). */
export interface BuiltPage {
  pathname: string;
  sourcePage: string;
  filePath: string;
}

/** Writes the manifest, its programs and its encoders into `<distDir>/tsx2rs/`. */
export function writeManifest(
  dir: string,
  projectDir: string,
  app: App,
  built: BuiltPage[],
  meta: { nextVersion: string; buildId: string },
): Manifest {
  mkdirSync(join(dir, "programs"), { recursive: true });
  mkdirSync(join(dir, "encoders"), { recursive: true });
  const routes = app.routes.map((route, i): ManifestRoute => {
    const outputs = built.filter((b) => b.sourcePage === sourcePage(route.page));
    const base = {
      page: route.page,
      pathnames: outputs.map((o) => o.pathname).sort(),
      entrypoint:
        outputs[0] === undefined
          ? null
          : relative(projectDir, outputs[0].filePath).replaceAll("\\", "/"),
    };
    if (!route.compiled || route.program === undefined || route.encoder === undefined) {
      return { ...base, compiled: false, reasons: route.reasons };
    }
    const program = `programs/${String(i)}.bin`;
    const encoder = `encoders/${String(i)}.mjs`;
    writeFileSync(join(dir, program), route.program);
    writeFileSync(join(dir, encoder), route.encoder);
    return {
      ...base,
      compiled: true,
      reasons: [],
      program,
      encoder,
      ...(route.entry ? { entry: route.entry } : {}),
      callees: route.callees,
    };
  });
  const manifest: Manifest = { version: MANIFEST_VERSION, ...meta, routes };
  writeFileSync(join(dir, "manifest.json"), `${JSON.stringify(manifest, null, 2)}\n`);
  return manifest;
}

/** One line per route, as the build prints them. */
export function report(manifest: Manifest): string[] {
  return manifest.routes.map((route) =>
    route.compiled
      ? `[tsx2rs] ${route.page}: compiled`
      : `[tsx2rs] ${route.page}: falls back: ${route.reasons.join("; ")}`,
  );
}

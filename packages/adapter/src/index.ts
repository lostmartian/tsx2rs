/**
 * `@tsx2rs/adapter`: the Next.js Deployment Adapter (ADR-0017). Set it as `adapterPath` in
 * `next.config`:
 *
 * ```js
 * export default { adapterPath: import.meta.resolve("@tsx2rs/adapter") };
 * ```
 *
 * During `next build` it compiles the app's pages, has the bundler append each compiled
 * server module's data functions in the server bundle, and writes the manifest
 * (`<distDir>/tsx2rs/manifest.json`), printing whether each route compiled or why it falls
 * back.
 */
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { join, relative } from "node:path";
import { compileApp, type App } from "@tsx2rs/native";
import type { NextAdapter } from "next";
import { report, writeManifest, type NextBuild, type NextOutput } from "./manifest.ts";
import { findPages } from "./pages.ts";

const require = createRequire(import.meta.url);
const loader = require.resolve("@tsx2rs/adapter/loader");

/** Where the compile result waits for the loader (which runs in other processes) and
 * `onBuildComplete`: outside `distDir`, which Next.js cleans as the build starts. */
function buildFile(projectDir: string): string {
  return join(projectDir, "node_modules", ".cache", "tsx2rs", "build.json");
}

type Outputs = Parameters<NonNullable<NextAdapter["onBuildComplete"]>>[0]["outputs"];

/** Every build output, with paths relative to the project. */
function nextOutputs(outputs: Outputs, projectDir: string): NextOutput[] {
  const rel = (file: string | undefined) =>
    file === undefined ? null : relative(projectDir, file).replaceAll("\\", "/");
  const routes = [
    ...outputs.appPages,
    ...outputs.appRoutes,
    ...outputs.pages,
    ...outputs.pagesApi,
    ...(outputs.middleware ? [outputs.middleware] : []),
  ].map((o) => ({ type: o.type, id: o.id, pathname: o.pathname, file: rel(o.filePath) }));
  const prerenders = outputs.prerenders.map((o) => ({
    type: o.type,
    id: o.id,
    pathname: o.pathname,
    file: rel(o.fallback?.filePath),
    parent: o.parentOutputId,
  }));
  const statics = outputs.staticFiles.map((o) => ({
    type: o.type,
    id: o.id,
    pathname: o.pathname,
    file: rel(o.filePath),
  }));
  return [...routes, ...prerenders, ...statics];
}

/** The part of webpack's config the adapter touches. */
interface WebpackConfig {
  module: { rules: unknown[] };
}
type Webpack = (config: WebpackConfig, context: { isServer: boolean }) => WebpackConfig;

const adapter: NextAdapter = {
  name: "tsx2rs",

  modifyConfig(config, { phase, projectDir }) {
    if (phase !== "phase-production-build") return config;
    const app = compileApp(projectDir, findPages(projectDir));
    const build = buildFile(projectDir);
    mkdirSync(join(build, ".."), { recursive: true });
    writeFileSync(
      build,
      JSON.stringify(app, (_, v: unknown) => (Buffer.isBuffer(v) ? v.toString("base64") : v)),
    );
    const options = { root: projectDir, build };
    // Server bundles only (`browser` is the client), and only the app's own files.
    const condition = { all: [{ not: "browser" as const }, { not: "foreign" as const }] };
    const rule = { loaders: [{ loader, options }], condition };
    const previous = config.webpack as Webpack | null | undefined;
    return {
      ...config,
      turbopack: {
        ...config.turbopack,
        rules: { ...config.turbopack.rules, "*.tsx": rule, "*.jsx": rule },
      },
      webpack: (webpackConfig: WebpackConfig, context: { isServer: boolean }) => {
        const out = previous ? previous(webpackConfig, context) : webpackConfig;
        if (context.isServer) {
          out.module.rules.push({
            test: /\.(tsx|jsx)$/,
            exclude: /node_modules/,
            use: [{ loader, options }],
          });
        }
        return out;
      },
    };
  },

  onBuildComplete({ outputs, routing, config, distDir, projectDir, buildId, nextVersion }) {
    const saved = JSON.parse(readFileSync(buildFile(projectDir), "utf8")) as App;
    for (const route of saved.routes) {
      if (route.program !== undefined)
        route.program = Buffer.from(route.program as unknown as string, "base64");
    }
    const next: NextBuild = {
      basePath: config.basePath,
      i18n: config.i18n ?? null,
      routing,
      outputs: nextOutputs(outputs, projectDir),
      images: config.images,
      generateEtags: config.generateEtags,
      compress: config.compress,
    };
    const manifest = writeManifest(join(distDir, "tsx2rs"), projectDir, saved, outputs.appPages, {
      nextVersion,
      buildId,
      next,
    });
    for (const line of report(manifest)) console.log(line);
  },
};

export default adapter;

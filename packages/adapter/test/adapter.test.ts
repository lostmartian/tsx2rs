import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { afterAll, describe, expect, it, vi } from "vitest";
import type { NextConfigComplete } from "next/dist/server/config-shared.js";
import adapter from "../src/index.ts";
import loader from "../src/loader.ts";
import { MANIFEST_VERSION, report, type Manifest } from "../src/manifest.ts";
import { appDir, findPages, sourcePage } from "../src/pages.ts";

const root = mkdtempSync(join(tmpdir(), "tsx2rs-adapter-"));
afterAll(() => {
  rmSync(root, { recursive: true, force: true });
});

function write(path: string, source: string) {
  mkdirSync(dirname(join(root, path)), { recursive: true });
  writeFileSync(join(root, path), source);
}

write("app/layout.tsx", "export default function L({ children }) { return children; }\n");
write("app/page.tsx", "export default function Home() { return <h1>home</h1>; }\n");
write("app/(shop)/p/[n]/page.jsx", "export default function P() { return <p>p</p>; }\n");
write("app/c/page.tsx", '"use client";\nexport default function C() { return <p>c</p>; }\n');
write("app/node_modules/x/page.tsx", "ignored\n");

describe("pages", () => {
  it("finds every page file of the app, sorted", () => {
    expect(appDir(root)).toBe("app");
    expect(findPages(root)).toEqual([
      "app/(shop)/p/[n]/page.jsx",
      "app/c/page.tsx",
      "app/page.tsx",
    ]);
    expect(findPages(join(root, "missing"))).toEqual([]);
  });

  it("names a page as Next.js's outputs do", () => {
    expect(sourcePage("app/(shop)/p/[n]/page.jsx")).toBe("/(shop)/p/[n]/page");
    expect(sourcePage("src/app/page.tsx")).toBe("/page");
  });
});

describe("the adapter", () => {
  const config = { distDir: ".next", turbopack: { rules: { "*.svg": { loaders: ["svg"] } } } };
  const ctx = { phase: "phase-production-build" as const, nextVersion: "16.4.5", projectDir: root };

  it("leaves the config alone outside production builds", async () => {
    const same = await adapter.modifyConfig?.(config as unknown as NextConfigComplete, {
      ...ctx,
      phase: "phase-development-server" as const,
    });
    expect(same).toBe(config);
  });

  it("compiles the app and adds the loader for server bundles only", async () => {
    const modified = (await adapter.modifyConfig?.(
      config as unknown as NextConfigComplete,
      ctx,
    )) as unknown as {
      turbopack: { rules: Record<string, { condition?: unknown; loaders?: unknown[] }> };
      webpack: (c: unknown, x: { isServer: boolean }) => { module: { rules: unknown[] } };
    };
    // The app's own rules stay.
    expect(modified.turbopack.rules["*.svg"]).toEqual({ loaders: ["svg"] });
    expect(modified.turbopack.rules["*.tsx"]?.condition).toEqual({
      all: [{ not: "browser" }, { not: "foreign" }],
    });
    // webpack: the server compiler only.
    const server = modified.webpack({ module: { rules: [] } }, { isServer: true });
    const client = modified.webpack({ module: { rules: [] } }, { isServer: false });
    expect(server.module.rules).toHaveLength(1);
    expect(client.module.rules).toHaveLength(0);
  });

  it("writes the manifest and reports every route", async () => {
    const log = vi.spyOn(console, "log").mockImplementation(() => undefined);
    const distDir = join(root, ".next");
    const built = (page: string, pathname: string) => ({
      pathname,
      sourcePage: page,
      filePath: join(distDir, `server/app${page}.js`),
    });
    await adapter.onBuildComplete?.({
      outputs: {
        appPages: [built("/page", "/"), built("/page", "/index.rsc"), built("/c/page", "/c")],
        appRoutes: [],
        pages: [],
        pagesApi: [],
        prerenders: [
          {
            type: "PRERENDER",
            id: "/c",
            pathname: "/c",
            parentOutputId: "/c",
            fallback: { filePath: join(distDir, "server/app/c.html") },
          },
        ],
        staticFiles: [
          {
            type: "STATIC_FILE",
            id: "s",
            pathname: "/_next/static/x.js",
            filePath: join(distDir, "static/x.js"),
          },
        ],
      },
      routing: { beforeMiddleware: [] },
      config: {
        basePath: "",
        images: { formats: ["image/webp"] },
        generateEtags: true,
        compress: true,
      },
      distDir,
      projectDir: root,
      buildId: "b1",
      nextVersion: "16.4.5",
    } as unknown as Parameters<NonNullable<typeof adapter.onBuildComplete>>[0]);
    const manifest = JSON.parse(
      readFileSync(join(distDir, "tsx2rs/manifest.json"), "utf8"),
    ) as Manifest;
    expect(manifest.version).toBe(MANIFEST_VERSION);
    expect(manifest.buildId).toBe("b1");
    // What the runtime routes with, paths relative to the project.
    expect(manifest.next).toMatchObject({
      basePath: "",
      i18n: null,
      routing: { beforeMiddleware: [] },
      images: { formats: ["image/webp"] },
    });
    expect(manifest.next.outputs).toContainEqual({
      type: "PRERENDER",
      id: "/c",
      pathname: "/c",
      file: ".next/server/app/c.html",
      parent: "/c",
    });
    expect(manifest.next.outputs).toContainEqual({
      type: "STATIC_FILE",
      id: "s",
      pathname: "/_next/static/x.js",
      file: ".next/static/x.js",
    });
    const home = manifest.routes.find((r) => r.page === "app/page.tsx");
    expect(home).toMatchObject({
      pathnames: ["/", "/index.rsc"],
      entrypoint: ".next/server/app/page.js",
      compiled: true,
      entry: { module: "app/page.tsx", name: "__tsx2rs_data_Home" },
    });
    const program = readFileSync(join(distDir, "tsx2rs", home?.program ?? ""));
    expect(program.subarray(0, 8).toString("latin1")).toBe("TSX2RSP\0");
    expect(readFileSync(join(distDir, "tsx2rs", home?.encoder ?? ""), "utf8")).toContain(
      "export function encode",
    );
    // A page Next.js didn't build has no entrypoint.
    expect(
      manifest.routes.find((r) => r.page === "app/(shop)/p/[n]/page.jsx")?.entrypoint,
    ).toBeNull();
    expect(log).toHaveBeenCalledWith("[tsx2rs] app/page.tsx: compiled");
    expect(log).toHaveBeenCalledWith(
      '[tsx2rs] app/c/page.tsx: falls back: app/c/page.tsx is a client page (`"use client"`)',
    );
    log.mockRestore();
  });

  it("has the loader append a compiled module's code, and pass other modules through", () => {
    const build = join(root, "node_modules/.cache/tsx2rs/build.json");
    const run = (path: string, source: string) =>
      loader.call({ resourcePath: join(root, path), getOptions: () => ({ root, build }) }, source);
    const page = run("app/page.tsx", "SOURCE");
    expect(page.startsWith("SOURCE\n")).toBe(true);
    expect(page).toContain('set("app/page.tsx", { __tsx2rs_data_Home });');
    // Layouts aren't in a page's module graph: they pass through (the wrapper is captured, 7.3).
    expect(run("app/layout.tsx", "LAYOUT")).toBe("LAYOUT");
    expect(run("app/c/page.tsx", "CLIENT")).toBe("CLIENT");
  });
});

describe("report", () => {
  it("prints one line per route", () => {
    const manifest: Manifest = {
      version: MANIFEST_VERSION,
      nextVersion: "16.4.5",
      buildId: "b",
      next: {
        basePath: "",
        i18n: null,
        routing: {},
        outputs: [],
        images: {},
        generateEtags: true,
        compress: true,
        htmlLimitedBots: null,
      },
      routes: [
        { page: "a", pathnames: [], entrypoint: null, compiled: true, reasons: [] },
        { page: "b", pathnames: [], entrypoint: null, compiled: false, reasons: ["x", "y"] },
      ],
    };
    expect(report(manifest)).toEqual(["[tsx2rs] a: compiled", "[tsx2rs] b: falls back: x; y"]);
  });
});

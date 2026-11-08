/**
 * The adapter in a real `next build` (ADR-0017), with Turbopack and with webpack: the build
 * reports every route, writes the manifest, puts the data functions into Next.js's server
 * bundle where a runtime finds them, and leaves the client bundles exactly as Next.js alone
 * builds them.
 */
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { readdirSync, readFileSync, rmSync } from "node:fs";
import { join, relative } from "node:path";
import { describe, expect, it } from "vitest";
import type { Manifest } from "../src/manifest.ts";

const app = join(import.meta.dirname, "app");
const next = join(app, "node_modules", ".bin", "next");

function build(args: string[], env: Record<string, string> = {}): string {
  rmSync(join(app, ".next"), { recursive: true, force: true });
  return execFileSync(next, ["build", ...args], {
    cwd: app,
    env: { ...process.env, NEXT_TELEMETRY_DISABLED: "1", ...env },
    encoding: "utf8",
  });
}

function manifest(): Manifest {
  return JSON.parse(readFileSync(join(app, ".next/tsx2rs/manifest.json"), "utf8")) as Manifest;
}

interface Probe {
  registered: string[];
  results: { page: string; input?: string; notNative?: string }[];
}

function probe(): Probe {
  const out = execFileSync(process.execPath, [join(import.meta.dirname, "probe.ts")], {
    cwd: app,
    env: { ...process.env, NODE_ENV: "production" },
    encoding: "utf8",
  });
  return JSON.parse(out.trim().split("\n").at(-1) ?? "{}") as Probe;
}

/** The client chunks' contents by path (`.next/static/chunks`). */
function clientChunks(): Map<string, string> {
  const root = join(app, ".next/static/chunks");
  const out = new Map<string, string>();
  const walk = (dir: string) => {
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
      const path = join(dir, entry.name);
      if (entry.isDirectory()) walk(path);
      else
        out.set(
          relative(root, path),
          createHash("sha256").update(readFileSync(path)).digest("hex"),
        );
    }
  };
  walk(root);
  return out;
}

describe.each([
  ["Turbopack", []],
  ["webpack", ["--webpack"]],
] as const)("next build (%s)", (_, args) => {
  let output = "";
  it("reports every route", () => {
    output = build([...args]);
    expect(output).toContain("[tsx2rs] app/p/[n]/page.tsx: compiled");
    expect(output).toContain("[tsx2rs] app/q/[n]/page.tsx: compiled");
    expect(output).toContain(
      "[tsx2rs] app/client/page.tsx: falls back: app/client/page.tsx is a client page",
    );
  });

  it("writes the manifest", () => {
    const m = manifest();
    expect(m.version).toBe(2);
    expect(m.nextVersion).toBe("16.4.5");
    const page = m.routes.find((r) => r.page === "app/p/[n]/page.tsx");
    expect(page).toMatchObject({
      pathnames: ["/p/[n]", "/p/[n].rsc"],
      entrypoint: ".next/server/app/p/[n]/page.js",
      compiled: true,
      entry: { module: "app/p/[n]/page.tsx", name: "__tsx2rs_data_Page" },
      callees: [{ module: "app/components/price.tsx", name: "__tsx2rs_data_Price" }],
    });
    const program = readFileSync(join(app, ".next/tsx2rs", page?.program ?? ""));
    expect(program.subarray(0, 8).toString("latin1")).toBe("TSX2RSP\0");
  });

  it("puts the data functions where a runtime finds them, and they encode", () => {
    const { registered, results } = probe();
    // Server components only: not the client `Counter`.
    expect(registered).toEqual([
      "app/components/price.tsx",
      "app/p/[n]/page.tsx",
      "app/q/[n]/page.tsx",
    ]);
    // The page without client components encodes natively.
    expect(results.find((r) => r.page === "app/q/[n]/page.tsx")?.input?.length).toBeGreaterThan(0);
    // `<Counter />` is a client component: an island, which needs a resolver (ADR-0020).
    expect(results.find((r) => r.page === "app/p/[n]/page.tsx")?.notNative).toBe(
      "a client component element, not learned yet",
    );
  });
});

describe("client bundles", () => {
  it("are what Next.js builds without the adapter", () => {
    build([]);
    const withAdapter = clientChunks();
    build([], { TSX2RS_ADAPTER: "off" });
    const without = clientChunks();
    expect(withAdapter).toEqual(without);
    expect(withAdapter.size).toBeGreaterThan(0);
  });
});

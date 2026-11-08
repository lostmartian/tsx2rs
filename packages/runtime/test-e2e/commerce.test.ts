/**
 * Milestone 1's app, Vercel Commerce (tests/apps/commerce), built with `@tsx2rs/adapter` from its
 * one-line `next.config.ts` change, served by `next start` and by the tsx2rs server side by side.
 * Every page must be byte-identical; the routes tsx2rs serves itself say so (`x-tsx2rs`).
 */
import { execFileSync, spawn, type ChildProcess } from "node:child_process";
import { createServer } from "node:net";
import { rmSync } from "node:fs";
import { join } from "node:path";
import { afterAll, beforeAll, describe, expect, it } from "vitest";

const app = join(import.meta.dirname, "../../../tests/apps/commerce");
const next = join(app, "node_modules", ".bin", "next");
const env = { ...process.env, NODE_ENV: "production", NEXT_TELEMETRY_DISABLED: "1" };

/** Milestone 1's reference pages (doc/benchmarks/2026-10-25-engine-gate.md, lab repository). */
const REFERENCE = [
  "/product/acme-geometric-circles-t-shirt",
  "/product/acme-geometric-circles-t-shirt?color=Blue&size=M",
  "/product/acme-geometric-circles-t-shirt?color=Blue&size=XL",
  "/product/acme-geometric-circles-t-shirt?image=2",
  "/product/acme-hoodie",
  "/product/acme-sticker",
  "/product-streaming/acme-geometric-circles-t-shirt",
  "/product-streaming/acme-geometric-circles-t-shirt?color=Blue&size=M",
  "/product-streaming/acme-hoodie",
];
const OTHERS = [
  "/",
  "/about",
  "/terms-conditions",
  "/shipping-return-policy",
  "/search",
  "/search/shirts",
  "/nope",
];

function freePort(): Promise<number> {
  return new Promise((resolve) => {
    const s = createServer().listen(0, "127.0.0.1", () => {
      const address = s.address();
      s.close(() => {
        resolve(typeof address === "object" && address ? address.port : 0);
      });
    });
  });
}

async function serve(
  command: string,
  args: string[],
  extra: Record<string, string>,
  url: string,
): Promise<ChildProcess> {
  const child = spawn(command, args, { cwd: app, env: { ...env, ...extra }, stdio: "ignore" });
  for (let i = 0; i < 240; i++) {
    try {
      await fetch(url);
      return child;
    } catch {
      await new Promise((r) => setTimeout(r, 250));
    }
  }
  child.kill();
  throw new Error(`${command} didn't start`);
}

let nextServer: ChildProcess;
let ours: ChildProcess;
let nextUrl = "";
let oursUrl = "";

beforeAll(async () => {
  rmSync(join(app, ".next"), { recursive: true, force: true });
  execFileSync(next, ["build"], { cwd: app, env, stdio: "ignore" });
  const [a, b] = [await freePort(), await freePort()];
  nextUrl = `http://127.0.0.1:${String(a)}`;
  oursUrl = `http://127.0.0.1:${String(b)}`;
  nextServer = await serve(next, ["start", "-p", String(a), "-H", "127.0.0.1"], {}, nextUrl);
  ours = await serve(
    process.execPath,
    [join(import.meta.dirname, "serve.ts")],
    { PORT: String(b) },
    oursUrl,
  );
});

afterAll(() => {
  nextServer.kill();
  ours.kill();
});

async function page(base: string, path: string) {
  const res = await fetch(base + path, { redirect: "manual" });
  return {
    status: res.status,
    type: res.headers.get("content-type"),
    compiled: res.headers.get("x-tsx2rs"),
    body: await res.text(),
  };
}

describe("Commerce", () => {
  it("serves a CMS page itself once it has learned it from Next.js", async () => {
    await page(oursUrl, "/about");
    const [n, o] = [await page(nextUrl, "/about"), await page(oursUrl, "/about")];
    expect(o.compiled).toBe("compiled");
    expect(o.body).toBe(n.body);
  });

  it.each([...REFERENCE, ...OTHERS])(
    "answers %s byte for byte as next start does (twice)",
    async (path) => {
      for (let i = 0; i < 2; i++) {
        const [n, o] = [await page(nextUrl, path), await page(oursUrl, path)];
        expect({ ...o, compiled: null }).toEqual({ ...n, compiled: null });
      }
    },
  );

  it("says why it doesn't serve the other routes itself", async () => {
    const states = (await (await fetch(`${oursUrl}/__tsx2rs`)).json()) as Record<
      string,
      { state: string; reason?: string }
    >;
    // Each CMS page's footer marks its own link: the wrapper depends on the URL, so each URL
    // keeps its own (ADR-0023), and the pages above were served from it.
    expect(states["/[page]"]).toMatchObject({ state: "served", perRequest: true });
    for (const path of ["/about", "/terms-conditions", "/shipping-return-policy"]) {
      const [n, o] = [await page(nextUrl, path), await page(oursUrl, path)];
      expect(o.compiled).toBe("compiled");
      expect(o.body).toBe(n.body);
    }
    // Client components are islands, learned from Next.js's responses next (ADR-0020).
    for (const route of ["/", "/product/[handle]", "/product-streaming/[handle]"]) {
      expect(states[route]).toMatchObject({
        state: "refused",
        reason: expect.stringMatching(/^tsx2rs can't render it: a /) as string,
      });
    }
  });
});

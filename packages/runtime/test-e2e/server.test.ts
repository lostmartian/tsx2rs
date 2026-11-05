/**
 * The tsx2rs server against `next start`, on the same build of the adapter's test app
 * (packages/adapter/test-e2e/app): every request tsx2rs doesn't serve gets Next.js's own answer,
 * byte for byte.
 */
import { execFileSync, spawn, type ChildProcess } from "node:child_process";
import { createServer } from "node:net";
import { readdirSync, rmSync } from "node:fs";
import { join } from "node:path";
import { afterAll, beforeAll, describe, expect, it } from "vitest";

const app = join(import.meta.dirname, "../../adapter/test-e2e/app");
const next = join(app, "node_modules", ".bin", "next");
const env = { ...process.env, NODE_ENV: "production", NEXT_TELEMETRY_DISABLED: "1" };

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

/** Starts `command` and waits until `url` answers. */
async function serve(
  command: string,
  args: string[],
  extra: Record<string, string>,
  url: string,
): Promise<ChildProcess> {
  const child = spawn(command, args, { cwd: app, env: { ...env, ...extra }, stdio: "ignore" });
  for (let i = 0; i < 120; i++) {
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

async function both(path: string, headers: Record<string, string> = {}) {
  const get = async (base: string) => {
    const res = await fetch(base + path, { headers, redirect: "manual" });
    return {
      status: res.status,
      type: res.headers.get("content-type"),
      cacheControl: res.headers.get("cache-control"),
      // Locations are absolute in fetch; compare their path.
      location: res.headers.get("location")?.replace(base, ""),
      body: await res.text(),
    };
  };
  return { next: await get(nextUrl), ours: await get(oursUrl) };
}

describe("the tsx2rs server, for what it doesn't compile", () => {
  const chunk = () =>
    `/_next/static/chunks/${readdirSync(join(app, ".next/static/chunks")).find((f) => f.endsWith(".js")) ?? ""}`;

  it.each([
    ["a dynamic page", "/q/3"],
    ["a dynamic page with a query string", "/q/3?x=1&y=%3Cb%3E"],
    ["a page with a client component", "/p/5"],
    ["a prerendered page", "/client"],
    ["a missing page (404)", "/nope"],
    ["a public file", "/robots.txt"],
  ])("answers %s as next start does", async (_, path) => {
    const { next, ours } = await both(path);
    expect(ours).toEqual(next);
  });

  it("serves static chunks as next start does", async () => {
    const { next, ours } = await both(chunk());
    expect(ours).toEqual(next);
    expect(ours.cacheControl).toBe("public, max-age=31536000, immutable");
  });

  it("redirects a client navigation without `_rsc` as next start does", async () => {
    const { next, ours } = await both("/q/3", { rsc: "1" });
    expect(ours).toEqual(next);
    expect(ours.status).toBe(307);
  });
});

describe("the tsx2rs server, for a compiled route", () => {
  /** A response's headers, without those of its transfer (and tsx2rs's marker). */
  const headers = async (base: string, path: string) => {
    const res = await fetch(base + path);
    const out: Record<string, string> = {};
    res.headers.forEach((value, name) => {
      if (
        ![
          "date",
          "connection",
          "keep-alive",
          "transfer-encoding",
          "content-length",
          "x-tsx2rs",
        ].includes(name)
      ) {
        out[name] = value;
      }
    });
    return { headers: out, compiled: res.headers.get("x-tsx2rs"), body: await res.text() };
  };
  const state = async () =>
    (await (await fetch(`${oursUrl}/__tsx2rs`)).json()) as Record<
      string,
      { state: string; reason?: string }
    >;

  it("learns Next.js's wrapper, confirms it, then serves the page itself, byte for byte", async () => {
    // Earlier requests (`/q/3`) taught it the wrapper; one with other params confirms it.
    await both("/q/7");
    expect((await state())["/q/[n]"]).toMatchObject({ state: "served" });
    for (const path of ["/q/12345678901234567890", "/q/a%20b%26c", "/q/9?x=1&y=%3Cb%3E", "/q/3"]) {
      const [n, ours] = [await headers(nextUrl, path), await headers(oursUrl, path)];
      expect(ours.compiled).toBe("compiled");
      expect(ours.body).toBe(n.body);
      expect(ours.headers).toEqual(n.headers);
    }
  });

  it("answers HEAD for a served page with the page's headers and no body", async () => {
    const res = await fetch(`${oursUrl}/q/4`, { method: "HEAD" });
    expect(res.headers.get("x-tsx2rs")).toBe("compiled");
    expect(res.headers.get("content-type")).toBe("text/html; charset=utf-8");
    expect(await res.text()).toBe("");
  });

  it("learns a client component's island from Next.js, then serves the page itself", async () => {
    // `/p/5` was answered by Next.js above, and taught the route `<Counter />` for that request.
    expect((await state())["/p/[n]"]).toMatchObject({ state: "learned" });
    for (const path of ["/p/5", "/p/6", "/p/6"]) {
      const [n, ours] = [await headers(nextUrl, path), await headers(oursUrl, path)];
      expect(ours.body).toBe(n.body);
      expect(ours.headers).toEqual(n.headers);
    }
    // `/p/6`'s island was learned from Next.js's response, which also confirmed the wrapper.
    expect((await state())["/p/[n]"]).toMatchObject({ state: "served" });
    expect((await headers(oursUrl, "/p/5")).compiled).toBe("compiled");
    expect((await headers(oursUrl, "/p/6")).compiled).toBe("compiled");
  });
});

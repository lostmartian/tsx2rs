import { readFileSync } from "node:fs";
import { createServer, type IncomingMessage, type Server, type ServerResponse } from "node:http";
import { join } from "node:path";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { Route, type CompiledRoute, type Renderer } from "../src/compiled.js";
import type { Islands } from "../src/encode.js";
import { IslandCache, Miss, learnIslands, placeholders } from "../src/islands.js";

/**
 * Real Next.js 16.4.3 documents for `app/p/[n]/page.tsx`, whose `<Counter />` is a client
 * component (see test/next/README.md).
 */
const doc = (n: string) => readFileSync(join(import.meta.dirname, "next", `p-${n}.html`), "utf8");

/** What Next.js rendered for `<Counter />`, and its reference. */
const COUNTER = {
  ref: "$L15",
  before: '<button type="button"><p class="price">0.00<!-- --> <span>USD</span></p></button>',
  after: "",
};

/** The page as the engine renders it, with `islands` for `<Counter />` (no props, no children). */
function page(n: string, islands: Islands): { name: string; html: string; flight: string } {
  const counter = islands(0, {});
  const price = Number(n).toFixed(2);
  return {
    name: "page",
    html: `<main><h1>Product <!-- -->${n}</h1><p class="price">${price}<!-- --> <span>USD</span></p>${counter.before}</main>`,
    flight:
      `["$","main",null,{"children":[["$","h1",null,{"children":["Product ","${n}"]}],` +
      `["$","p",null,{"className":"price","children":["${price}"," ",["$","span",null,{"children":"USD"}]]}],` +
      `["$","${counter.ref}",null,{}]]}]`,
  };
}

describe("placeholders", () => {
  it("answers each island with markers and a reference of its own, and records the calls", () => {
    const { islands, calls } = placeholders();
    expect(islands(3, { href: "/" })).toEqual({
      ref: "$L0",
      before: "<!--tsx2rs:0:b-->",
      after: "<!--tsx2rs:0:a-->",
    });
    expect(islands(3, { href: "/x" }).ref).toBe("$L1");
    expect(calls).toEqual([
      { index: 3, props: '{"href":"/"}' },
      { index: 3, props: '{"href":"/x"}' },
    ]);
  });
});

describe("learnIslands", () => {
  it("learns an island's HTML and reference from Next.js's response", () => {
    const { islands, calls } = placeholders();
    const learned = learnIslands(doc("5"), page("5", islands), calls);
    // The page as Next.js wrote it: what the wrapper is then captured around.
    const { html, flight } = page("5", () => COUNTER);
    expect(learned).toEqual({ ok: true, islands: [COUNTER], html, flight });
    expect(doc("5")).toContain(learned.ok ? learned.html : "");
  });

  it("learns the HTML on both sides of an island's children", () => {
    const document =
      '<body><nav><a href="/x">go <b>9</b></a>!</nav></body>' +
      `<script>self.__next_f.push(${JSON.stringify([1, '2:["$","nav",null,{"children":[["$","$L1f",null,{"href":"/x","children":["go ",["$","b",null,{"children":"9"}]]}],"!"]}]\n'])})</script>`;
    const { islands, calls } = placeholders();
    const link = islands(0, { href: "/x" });
    const html = `<nav>${link.before}go <b>9</b>${link.after}!</nav>`;
    const flight = `["$","nav",null,{"children":[["$","${link.ref}",null,{"href":"/x","children":["go ",["$","b",null,{"children":"9"}]]}],"!"]}]`;
    const learned = learnIslands(document, { html, flight }, calls);
    expect(learned).toMatchObject({
      ok: true,
      islands: [{ ref: "$L1f", before: '<a href="/x">', after: "</a>" }],
      html: '<nav><a href="/x">go <b>9</b></a>!</nav>',
    });
  });

  it("refuses what it can't place exactly once", () => {
    const { islands, calls } = placeholders();
    const fragment = page("5", islands);
    // Another page's response: the page's own text isn't there.
    expect(learnIslands(doc("6"), fragment, calls)).toEqual({
      ok: false,
      reason: "the page with its islands found 0 times in Next.js's response, not once",
    });
    // Markers that aren't the islands rendered.
    expect(learnIslands(doc("5"), fragment, [...calls, ...calls])).toMatchObject({
      ok: false,
      reason: "the page's island markers aren't the islands it rendered",
    });
    expect(
      learnIslands(
        doc("5"),
        { ...fragment, flight: fragment.flight.replace('"$L0"', '"x"') },
        calls,
      ),
    ).toMatchObject({
      ok: false,
      reason: "the page's Flight references aren't the islands it rendered",
    });
    expect(
      learnIslands(
        doc("5"),
        { ...fragment, flight: fragment.flight.replace("Product", "P") },
        calls,
      ),
    ).toMatchObject({
      ok: false,
      reason:
        "the page's Flight row with its islands found 0 times in Next.js's response, not once",
    });
    expect(
      learnIslands(
        doc("5"),
        { ...fragment, html: `${fragment.html.slice(0, 0)}<!--tsx2rs:0:b-->` },
        calls,
      ),
    ).toMatchObject({ ok: false, reason: "the page starts with an island" });
  });
});

describe("IslandCache", () => {
  it("answers a request with what it learned for it, in order, and misses anything else", () => {
    const cache = new IslandCache();
    cache.set("/p/5", [{ index: 0, props: "{}" }], [COUNTER]);
    expect(cache.resolver("/p/5")(0, {})).toBe(COUNTER);
    // Another request, other props, another component, one island more.
    expect(() => cache.resolver("/p/6")(0, {})).toThrow(Miss);
    expect(() => cache.resolver("/p/5")(0, { a: 1 })).toThrow(Miss);
    expect(() => cache.resolver("/p/5")(1, {})).toThrow(Miss);
    const resolve = cache.resolver("/p/5");
    resolve(0, {});
    expect(() => resolve(0, {})).toThrow(Miss);
  });
});

describe("a compiled route with an island", () => {
  const HEADERS = { "content-type": "text/html; charset=utf-8", "x-powered-by": "Next.js" };
  let nextCalls = 0;
  async function next(req: IncomingMessage, res: ServerResponse) {
    nextCalls++;
    const n = (req.url ?? "").split("/")[2] ?? "";
    res.writeHead(200, HEADERS);
    res.end(doc(n));
    return Promise.resolve();
  }
  // Async, so a `Miss` thrown by the resolver rejects, as the real renderer's does.
  // eslint-disable-next-line @typescript-eslint/require-await
  const renderer: Renderer = async (request, islands) => {
    if (!islands) throw new Error("no resolver");
    return [page(request.params["n"] ?? "", islands)];
  };

  const definition: CompiledRoute = {
    page: "app/p/[n]/page.tsx",
    pathnames: ["/p/[n]"],
    entrypoint: "",
    program: "",
    encoder: "",
    entry: { module: "", name: "" },
    callees: [],
  };
  const route = new Route(definition, renderer);
  let server: Server;
  let base = "";
  beforeAll(async () => {
    server = createServer((req, res) => {
      const url = new URL(req.url ?? "/", "http://localhost");
      const n = url.pathname.split("/")[2] ?? "";
      void route.handle(
        req,
        res,
        { params: { n }, pathname: url.pathname, search: url.search, query: {} },
        next,
      );
    });
    await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
    const a = server.address();
    base = `http://127.0.0.1:${String(typeof a === "object" && a ? a.port : 0)}`;
  });
  afterAll(() => {
    server.close();
  });

  it("learns the island from Next.js, then serves the request itself, byte for byte", async () => {
    const first = await fetch(`${base}/p/5`);
    expect(first.headers.get("x-tsx2rs")).toBeNull();
    expect(await first.text()).toBe(doc("5"));
    expect(route.state).toEqual({ state: "learned", params: { n: "5" } });
    const second = await fetch(`${base}/p/5`);
    expect(second.headers.get("x-tsx2rs")).toBe("compiled");
    expect(await second.text()).toBe(doc("5"));
    expect(nextCalls).toBe(1);
  });

  it("learns another request's islands from Next.js, and confirms the wrapper with it", async () => {
    const res = await fetch(`${base}/p/6`);
    expect(res.headers.get("x-tsx2rs")).toBeNull();
    expect(await res.text()).toBe(doc("6"));
    expect(route.state).toEqual({ state: "served" });
    const again = await fetch(`${base}/p/6`);
    expect(again.headers.get("x-tsx2rs")).toBe("compiled");
    expect(await again.text()).toBe(doc("6"));
    expect(nextCalls).toBe(2);
  });
});

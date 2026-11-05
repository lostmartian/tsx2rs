import { readFileSync } from "node:fs";
import { createServer, type IncomingMessage, type Server, type ServerResponse } from "node:http";
import { join } from "node:path";
import { createGzip } from "node:zlib";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { Route, type CompiledRoute, type Renderer, type Compress } from "../src/compiled.js";
import { decision } from "../src/server.js";

/** Real Next.js 16.4.3 documents for `app/q/[n]/page.tsx` (see test/next/README.md). */
const files = [
  ["3", "q-3"],
  ["7", "q-7"],
  ["1234567890123456789012345678901234567890", "q-long"],
  ["a%20b%26c", "q-escaped"],
] as const;
const docs: Record<string, string> = Object.fromEntries(
  files.map(([n, file]) => [
    n,
    readFileSync(join(import.meta.dirname, "next", `${file}.html`), "utf8"),
  ]),
);

/** What the engine renders for the page (as in capture.test.ts). */
const page: Renderer = (request) => {
  const n = request.params["n"] ?? "";
  if (n === "throws") return Promise.reject(new Error("a value the engine can't take natively"));
  const price = Number(n).toFixed(2);
  return Promise.resolve([
    {
      name: "page",
      html: `<main><h1>Price <!-- -->${n}</h1><p class="price">${price}<!-- --> <span>EUR</span></p></main>`,
      flight: JSON.stringify([
        "$",
        "main",
        null,
        {
          children: [
            ["$", "h1", null, { children: ["Price ", n] }],
            [
              "$",
              "p",
              null,
              {
                className: "price",
                children: [price, " ", ["$", "span", null, { children: "EUR" }]],
              },
            ],
          ],
        },
      ]),
    },
  ]);
};

const HEADERS = {
  vary: "rsc, next-router-state-tree, next-router-prefetch, next-router-segment-prefetch, Accept-Encoding",
  "x-powered-by": "Next.js",
  "cache-control": "private, no-cache, no-store, max-age=0, must-revalidate",
  "content-type": "text/html; charset=utf-8",
};

/** Next.js, as the route sees it: writes the saved document for `n`, in two chunks. */
let nextCalls = 0;
let acceptEncoding: string | undefined;
async function next(req: IncomingMessage, res: ServerResponse) {
  nextCalls++;
  acceptEncoding = req.headers["accept-encoding"];
  const n = decodeURIComponent((req.url ?? "").split("/")[2] ?? "")
    .replace(/ /g, "%20")
    .replace(/&/g, "%26");
  const doc = docs[n];
  if (!doc) {
    res.writeHead(404, { "content-type": "text/plain" }).end("not found");
    return;
  }
  res.writeHead(200, HEADERS);
  res.write(doc.slice(0, 100));
  await new Promise((r) => setTimeout(r, 1));
  res.end(Buffer.from(doc.slice(100)));
}

const definition: CompiledRoute = {
  page: "app/q/[n]/page.tsx",
  pathnames: ["/q/[n]"],
  entrypoint: "",
  program: "",
  encoder: "",
  entry: { module: "", name: "" },
  callees: [],
};

function serve(route: Route): Promise<{ server: Server; url: string }> {
  const server = createServer((req, res) => {
    const url = new URL(req.url ?? "/", "http://localhost");
    const n = url.pathname.split("/")[2] ?? "";
    void route.handle(
      req,
      res,
      { params: { n }, pathname: url.pathname, search: url.search, query: {} },
      next,
    );
  });
  return new Promise((resolve) => {
    server.listen(0, "127.0.0.1", () => {
      const a = server.address();
      resolve({
        server,
        url: `http://127.0.0.1:${String(typeof a === "object" && a ? a.port : 0)}`,
      });
    });
  });
}

describe("a compiled route", () => {
  const route = new Route(definition, page);
  let base = "";
  let server: Server;
  beforeAll(async () => {
    ({ server, url: base } = await serve(route));
  });
  afterAll(() => {
    server.close();
  });
  const get = async (path: string, init?: RequestInit) => {
    const res = await fetch(base + path, init);
    return {
      compiled: res.headers.get("x-tsx2rs"),
      type: res.headers.get("content-type"),
      body: await res.text(),
    };
  };

  it("lets Next.js answer while it learns, without compression, and keeps the document", async () => {
    const calls = nextCalls;
    const r = await get("/q/3", { headers: { "accept-encoding": "gzip" } });
    expect(r).toEqual({ compiled: null, type: HEADERS["content-type"], body: docs["3"] });
    expect(nextCalls).toBe(calls + 1);
    expect(acceptEncoding).toBeUndefined();
    expect(route.state).toEqual({ state: "learned", params: { n: "3" } });
  });

  it("serves the learned params itself, and Next.js the others until they confirm the wrapper", async () => {
    expect(await get("/q/3")).toEqual({
      compiled: "compiled",
      type: HEADERS["content-type"],
      body: docs["3"],
    });
    const calls = nextCalls;
    expect((await get("/q/7")).compiled).toBeNull();
    expect(nextCalls).toBe(calls + 1);
    expect(route.state).toEqual({ state: "served" });
  });

  it("then serves any params byte for byte, and HEAD without a body", async () => {
    for (const n of ["1234567890123456789012345678901234567890", "a%20b%26c"]) {
      expect(await get(`/q/${n}`)).toEqual({
        compiled: "compiled",
        type: HEADERS["content-type"],
        body: docs[n],
      });
    }
    expect(await get("/q/7", { method: "HEAD" })).toEqual({
      compiled: "compiled",
      type: HEADERS["content-type"],
      body: "",
    });
  });

  it("refuses the route when tsx2rs can't render a request, and Next.js serves it from then on", async () => {
    const calls = nextCalls;
    expect((await get("/q/throws")).compiled).toBeNull();
    expect(route.state).toEqual({
      state: "refused",
      reason: "tsx2rs can't render it: a value the engine can't take natively",
    });
    expect((await get("/q/3")).compiled).toBeNull();
    expect(nextCalls).toBe(calls + 2);
  });
});

describe("a compiled route's refusals", () => {
  it("refuses a document it can't learn from, and doesn't learn from errors", async () => {
    const wrong: Renderer = () =>
      Promise.resolve([{ name: "page", html: "<p>not it</p>", flight: "[]" }]);
    const route = new Route(definition, wrong);
    const { server, url } = await serve(route);
    await (await fetch(`${url}/q/missing`)).text();
    expect(route.state).toEqual({ state: "learning" });
    await (await fetch(`${url}/q/3`)).text();
    expect(route.state).toEqual({
      state: "refused",
      reason: "page's html found 0 times in Next.js's response, not once",
    });
    server.close();
  });

  it("refuses when Next.js's headers change between requests", async () => {
    const route = new Route(definition, page);
    const { server, url } = await serve(route);
    await (await fetch(`${url}/q/3`)).text();
    const original = HEADERS["cache-control"];
    HEADERS["cache-control"] = "public, max-age=60";
    await (await fetch(`${url}/q/7`)).text();
    HEADERS["cache-control"] = original;
    expect(route.state).toEqual({
      state: "refused",
      reason: "Next.js's headers differ between requests",
    });
    server.close();
  });

  it("refuses a confirmation that differs in ways the slots don't explain", async () => {
    const route = new Route(definition, page);
    const { server, url } = await serve(route);
    await (await fetch(`${url}/q/3`)).text();
    const doc = docs["7"] ?? "";
    docs["7"] = doc.replace('<html lang="en">', '<html lang="de">');
    await (await fetch(`${url}/q/7`)).text();
    docs["7"] = doc;
    expect(route.state).toMatchObject({ state: "refused" });
    server.close();
  });
});

describe("a served route's compression", () => {
  it("compresses as Next.js's middleware does, when it's on", async () => {
    let used = 0;
    // A stand-in for Next.js's `compression` middleware: gzips what the route writes.
    const compress: Compress = (_req, res) => {
      used++;
      const gzip = createGzip();
      const end = res.end.bind(res);
      const chunks: Buffer[] = [];
      gzip.on("data", (c: Buffer) => chunks.push(c));
      gzip.on("end", () => {
        end(Buffer.concat(chunks));
      });
      res.setHeader("content-encoding", "gzip");
      res.end = ((body?: unknown) => {
        if (body) gzip.end(body as string);
        else gzip.end();
        return res;
      }) as typeof res.end;
    };
    const route = new Route(definition, page, compress);
    const { server, url } = await serve(route);
    await (await fetch(`${url}/q/3`)).text();
    await (await fetch(`${url}/q/7`)).text();
    const res = await fetch(`${url}/q/1234567890123456789012345678901234567890`, {
      headers: { "accept-encoding": "gzip" },
    });
    expect(used).toBe(1);
    expect(res.headers.get("x-tsx2rs")).toBe("compiled");
    // fetch decodes the gzip itself.
    expect(res.headers.get("content-encoding")).toBe("gzip");
    expect(await res.text()).toBe(docs["1234567890123456789012345678901234567890"]);
    server.close();
  });
});

describe("decision", () => {
  const compiled = new Map([["/q/[n]", null]]);

  it("gives the params and query Next.js gives the page", () => {
    expect(
      decision(
        {
          resolvedPathname: "/q/[n]",
          routeMatches: { "1": "a%20b", "2": "", nxtPn: "a%20b" },
          invocationTarget: { pathname: "/q/a%20b", query: { x: "1", nxtPn: "a b", nxtIfoo: "y" } },
        },
        compiled,
      ),
    ).toEqual({ route: "/q/[n]", params: { n: "a%20b" }, query: { x: "1" } });
  });

  it("leaves to Next.js what isn't a compiled route, or has routing rules", () => {
    expect(decision({}, compiled)).toEqual({ fallback: "no route" });
    expect(decision({ resolvedPathname: "/other" }, compiled)).toEqual({
      fallback: "/other isn't compiled",
    });
    expect(
      decision(
        { resolvedPathname: "/q/[n]", resolvedHeaders: new Headers({ "x-custom": "1" }) },
        compiled,
      ),
    ).toEqual({ fallback: "routing rules apply (redirect, rewrite or headers)" });
    expect(
      decision(
        { resolvedPathname: "/q/[n]", redirect: { url: new URL("http://x/"), status: 308 } },
        compiled,
      ),
    ).toEqual({ fallback: "routing rules apply (redirect, rewrite or headers)" });
  });
});

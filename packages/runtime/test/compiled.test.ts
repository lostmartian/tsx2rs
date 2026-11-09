import { readFileSync } from "node:fs";
import {
  createServer,
  request,
  type IncomingMessage,
  type Server,
  type ServerResponse,
} from "node:http";
import { createRequire } from "node:module";
import { join } from "node:path";
import { gunzipSync, gzipSync, inflateSync } from "node:zlib";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import {
  Rechecks,
  Route,
  fragments,
  gzipsLikeNext,
  withVary,
  type CompiledRoute,
  type Renderer,
  type Compress,
} from "../src/compiled.js";
import { Decisions, decision, variant } from "../src/server.js";

/** Real Next.js 16.4.5 documents for `app/q/[n]/page.tsx` (see test/next/README.md). */
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
  // Compressed when the client asks, as `next start` does.
  if (acceptEncoding?.includes("gzip")) {
    const body = gzipSync(doc);
    res.writeHead(200, { ...HEADERS, "content-encoding": "gzip" });
    res.write(body.subarray(0, 50));
    await new Promise((r) => setTimeout(r, 1));
    res.end(body.subarray(50));
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

  it("lets Next.js answer while it learns, compressed as asked, and keeps the document", async () => {
    const calls = nextCalls;
    const r = await get("/q/3", { headers: { "accept-encoding": "gzip" } });
    // fetch decodes the gzip; the route learned from the decompressed copy.
    expect(r).toEqual({ compiled: null, type: HEADERS["content-type"], body: docs["3"] });
    expect(nextCalls).toBe(calls + 1);
    expect(acceptEncoding).toBe("gzip");
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

  it("re-checks every Nth request it would serve, and relearns when Next.js's answer changed", async () => {
    const route = new Route(definition, page, undefined, 2);
    const { server, url } = await serve(route);
    const get = async (path: string) => {
      const res = await fetch(url + path);
      return { compiled: res.headers.get("x-tsx2rs"), body: await res.text() };
    };
    await get("/q/3");
    await get("/q/7");
    expect(route.state).toEqual({ state: "served" });
    // Served, then re-checked: Next.js answers, the same.
    expect((await get("/q/3")).compiled).toBe("compiled");
    expect(await get("/q/3")).toEqual({ compiled: null, body: docs["3"] });
    expect(route.checks).toEqual({ compared: 1, differed: 0 });
    // A re-check Next.js answers with another status (a 404) isn't compared.
    const missing = docs["3"] ?? "";
    delete docs["3"];
    expect((await get("/q/3")).compiled).toBe("compiled");
    expect((await get("/q/3")).compiled).toBeNull();
    expect(route.checks).toEqual({ compared: 1, differed: 0 });
    docs["3"] = missing;
    // The layout changed since: the next re-check sees it, and the route relearns.
    const doc = docs["3"] ?? "";
    docs["3"] = doc.replace("<body>", "<body><b>new</b>");
    expect((await get("/q/3")).compiled).toBe("compiled");
    expect((await get("/q/3")).body).toBe(docs["3"]);
    expect(route.checks).toEqual({ compared: 2, differed: 1 });
    expect(route.state).toEqual({ state: "learning" });
    expect((await get("/q/3")).compiled).toBeNull();
    docs["3"] = doc;
    server.close();
  });

  it("hands a request whose page calls notFound() or redirect() to Next.js, and goes on", async () => {
    const thrown = (digest: string) => Object.assign(new Error(digest), { digest });
    const route = new Route(definition, (request) =>
      request.params["n"] === "7"
        ? Promise.reject(thrown("NEXT_HTTP_ERROR_FALLBACK;404"))
        : request.params["n"] === "3"
          ? Promise.reject(thrown("NEXT_REDIRECT;replace;/q/7;307;"))
          : page(request),
    );
    const { server, url } = await serve(route);
    const calls = nextCalls;
    expect(await (await fetch(`${url}/q/7`)).text()).toBe(docs["7"]);
    expect(await (await fetch(`${url}/q/3`)).text()).toBe(docs["3"]);
    expect(nextCalls).toBe(calls + 2);
    // Not refused, and nothing learned from those answers.
    expect(route.state).toEqual({ state: "learning" });
    server.close();
  });

  it("keeps a wrapper per request when it differs in ways the slots don't explain", async () => {
    const route = new Route(definition, page);
    const { server, url } = await serve(route);
    const get = async (path: string) => {
      const res = await fetch(url + path);
      return { compiled: res.headers.get("x-tsx2rs"), body: await res.text() };
    };
    const doc = docs["7"] ?? "";
    const german = doc.replace('<html lang="en">', '<html lang="de">');
    await get("/q/3");
    docs["7"] = german;
    await get("/q/7");
    // Not explained by the params: each URL keeps its own wrapper, once a URL's two responses
    // have shown that Next.js answers one request the same way.
    expect(route.state).toEqual({ state: "learning" });
    expect(await get("/q/7")).toEqual({ compiled: null, body: german });
    expect(route.state).toEqual({ state: "served", perRequest: true });
    expect(await get("/q/7")).toEqual({ compiled: "compiled", body: german });
    docs["7"] = doc;
    expect(await get("/q/3")).toEqual({ compiled: "compiled", body: docs["3"] });
    // A URL it hasn't seen goes to Next.js once, then it's served.
    const long = "/q/1234567890123456789012345678901234567890";
    expect((await get(long)).compiled).toBeNull();
    expect(await get(long)).toEqual({
      compiled: "compiled",
      body: docs["1234567890123456789012345678901234567890"],
    });
    server.close();
  });
});

describe("a served route's compression (ADR-0033)", () => {
  /** Next.js's own `compression` middleware, as `next start` applies it. */
  const real = (
    createRequire(import.meta.url)("next/dist/compiled/compression") as () => Compress
  )();

  /** Sends `headers` and `body` through `compress`, as raw bytes and headers. */
  function raw(url: string, path: string, acceptEncoding: string, method = "GET") {
    return new Promise<{ headers: Record<string, unknown>; body: Buffer }>((resolve, reject) => {
      const req = request(
        `${url}${path}`,
        { method, headers: { "accept-encoding": acceptEncoding } },
        (res) => {
          const chunks: Buffer[] = [];
          res.on("data", (c: Buffer) => chunks.push(c));
          res.on("end", () => {
            resolve({ headers: res.headers, body: Buffer.concat(chunks) });
          });
        },
      );
      req.on("error", reject);
      req.end();
    });
  }

  const decoded = (r: { headers: Record<string, unknown>; body: Buffer }) =>
    (r.headers["content-encoding"] === "gzip"
      ? gunzipSync(r.body)
      : r.headers["content-encoding"] === "deflate"
        ? inflateSync(r.body)
        : r.body
    ).toString();

  it("sends what Next.js's middleware sends, for every kind of Accept-Encoding", async () => {
    const route = new Route(definition, page, real);
    const ours = await serve(route);
    // The middleware alone, writing the same document with the headers the route learned.
    const doc = docs["1234567890123456789012345678901234567890"] ?? "";
    const theirs = createServer((req, res) => {
      real(req, res, () => undefined);
      res.writeHead(200, { ...HEADERS, "x-tsx2rs": "compiled" });
      res.end(doc);
    });
    await new Promise<void>((resolve) => theirs.listen(0, "127.0.0.1", resolve));
    const address = theirs.address();
    const url = `http://127.0.0.1:${String(typeof address === "object" && address ? address.port : 0)}`;
    const path = "/q/1234567890123456789012345678901234567890";
    // Learn and confirm the route first.
    for (const p of ["/q/3", "/q/7", path]) await raw(ours.url, p, "identity");
    const cases = [
      "gzip",
      "gzip, deflate, br",
      "gzip, deflate, br, zstd",
      "br, gzip",
      "deflate, gzip",
      "GZIP",
      "deflate",
      "identity",
      "",
      "*",
      "gzip;q=0.5, identity",
      "identity;q=1, gzip;q=0.5",
      "gzip;q=0",
      "br",
    ];
    for (const acceptEncoding of cases) {
      for (const method of ["GET", "HEAD"]) {
        const a = await raw(ours.url, path, acceptEncoding, method);
        const b = await raw(url, path, acceptEncoding, method);
        const label = `${method} ${acceptEncoding}`;
        expect(a.headers["x-tsx2rs"], label).toBe("compiled");
        expect(a.headers["content-encoding"], label).toBe(b.headers["content-encoding"]);
        expect(a.headers["vary"], label).toBe(b.headers["vary"]);
        expect(decoded(a), label).toBe(decoded(b));
        // Which compressed it: the engine's gzip header says OS "unknown" (255); zlib's doesn't.
        if (a.headers["content-encoding"] === "gzip") {
          // Only an explicit gzip without weights goes to the engine (`*` stays with the middleware).
          const engine =
            method === "GET" && !acceptEncoding.includes(";") && /gzip/i.test(acceptEncoding);
          expect(a.body[9] === 255, label).toBe(engine);
        }
      }
    }
    ours.server.close();
    theirs.close();
  });

  it("decides as the middleware does, and adds Vary as it does", () => {
    const html = { "content-type": "text/html; charset=utf-8" };
    expect(gzipsLikeNext("gzip, deflate, br", html, 2000)).toBe(true);
    // Left to the middleware: weights, no gzip, small, not HTML, no-transform, already encoded.
    expect(gzipsLikeNext("gzip;q=1", html, 2000)).toBe(false);
    expect(gzipsLikeNext("deflate, br", html, 2000)).toBe(false);
    expect(gzipsLikeNext(undefined, html, 2000)).toBe(false);
    expect(gzipsLikeNext(["gzip"], html, 2000)).toBe(false);
    expect(gzipsLikeNext("gzip", html, 1023)).toBe(false);
    expect(gzipsLikeNext("gzip", { "content-type": "application/json" }, 2000)).toBe(false);
    expect(gzipsLikeNext("gzip", { ...html, "cache-control": "no-transform" }, 2000)).toBe(false);
    expect(gzipsLikeNext("gzip", { ...html, "content-encoding": "br" }, 2000)).toBe(false);
    expect(withVary(undefined)).toBe("Accept-Encoding");
    expect(withVary("rsc")).toBe("rsc, Accept-Encoding");
    expect(withVary(["rsc", "accept-encoding"])).toBe("rsc, accept-encoding");
    expect(withVary("*")).toBe("*");
  });
});

describe("a re-check of a per-URL wrapper", () => {
  it("forgets that URL's wrapper when Next.js's answer changed, and keeps the others", async () => {
    const route = new Route(definition, page, undefined, 1);
    const { server, url } = await serve(route);
    const get = async (path: string) => {
      const res = await fetch(url + path);
      return { compiled: res.headers.get("x-tsx2rs"), body: await res.text() };
    };
    const doc = docs["7"] ?? "";
    // Per-URL wrappers: `/q/7`'s differs from `/q/3`'s, and repeats.
    docs["7"] = doc.replace('<html lang="en">', '<html lang="de">');
    for (const path of ["/q/3", "/q/7", "/q/7"]) await get(path);
    expect(route.state).toEqual({ state: "served", perRequest: true });
    // Every served request is re-checked (1): `/q/7` changed again, so its wrapper is forgotten.
    docs["7"] = doc.replace('<html lang="en">', '<html lang="fr">');
    expect((await get("/q/7")).body).toBe(docs["7"]);
    expect(route.checks).toEqual({ compared: 1, differed: 1 });
    expect(route.state).toEqual({ state: "served", perRequest: true });
    docs["7"] = doc;
    server.close();
  });
});

describe("Decisions", () => {
  it("keeps decisions per URL only when the routing rules read nothing else", () => {
    expect(Decisions.for({ beforeFiles: [{ has: [{ type: "cookie" }] }] }, null)).toBeUndefined();
    expect(Decisions.for({ afterFiles: [{ missing: [] }] }, null)).toBeUndefined();
    expect(Decisions.for({}, { locales: ["en"] })).toBeUndefined();
    const decisions = Decisions.for({ beforeMiddleware: [] }, null);
    expect(decisions).toBeDefined();
    const page = { route: "/q/[n]", params: { n: "3" }, query: { x: "1" } };
    const kept = decisions?.set("/q/3?x=1", page);
    expect(kept).toEqual(page);
    // Each request's own params and query.
    const again = decisions?.get("/q/3?x=1");
    expect(again).toEqual(page);
    expect(again).not.toBe(page);
    expect(again && "route" in again && again.params).not.toBe(page.params);
    expect(decisions?.set("/nope", { fallback: "no route" })).toEqual({ fallback: "no route" });
    expect(decisions?.get("/elsewhere")).toBeUndefined();
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

describe("variants", () => {
  const bots = (ua: string) =>
    ua.includes("Googlebot") ? "dom" : ua.includes("Twitterbot") ? "html" : undefined;

  it("key requests by the cookies they send and the kind of bot, nothing else", () => {
    expect(variant({}, bots)).toBe("");
    expect(variant({ "user-agent": "Mozilla/5.0" }, bots)).toBe("");
    expect(variant({ cookie: "b=2; a=1; b=3" }, bots)).toBe('{"cookies":["a","b"],"bot":""}');
    expect(variant({ "user-agent": "Googlebot/2.1" }, bots)).toBe('{"cookies":[],"bot":"dom"}');
    // The app's own `htmlLimitedBots` pattern wins.
    expect(variant({ "user-agent": "MyCrawler" }, bots, /MyCrawler/i)).toBe(
      '{"cookies":[],"bot":"html"}',
    );
  });

  it("learn and refuse one at a time: a refused variant leaves the others served", async () => {
    // A cart's content depends on the visitor, not on the params.
    let carts = 0;
    const route = new Route(definition, page);
    const server = createServer((req, res) => {
      const url = new URL(req.url ?? "/", "http://localhost");
      const n = url.pathname.split("/")[2] ?? "";
      const cart = (req.headers.cookie ?? "").includes("cart");
      const vary = cart ? '{"cookies":["cart"],"bot":""}' : "";
      // Next.js, for a visitor with a cart: the layout shows it, so the wrapper isn't the same.
      const nextFor = async (r: IncomingMessage, s: ServerResponse) => {
        if (!cart) {
          await next(r, s);
          return;
        }
        s.writeHead(200, HEADERS).end(
          (docs[n] ?? "").replace(
            "<body>",
            `<body><span>${String(++carts)} items in the cart</span>`,
          ),
        );
      };
      void route.handle(
        req,
        res,
        { variant: vary, params: { n }, pathname: url.pathname, search: "", query: {} },
        nextFor,
      );
    });
    await new Promise<void>((r) => server.listen(0, "127.0.0.1", r));
    const a = server.address();
    const url = `http://127.0.0.1:${String(typeof a === "object" && a ? a.port : 0)}`;
    const get = async (path: string, cookie?: string) => {
      const res = await fetch(url + path, cookie ? { headers: { cookie } } : {});
      await res.text();
      return res.headers.get("x-tsx2rs");
    };
    await get("/q/3");
    await get("/q/7");
    await get("/q/3", "cart=1");
    await get("/q/7", "cart=1");
    // The two carts differ: maybe the wrapper depends on the URL, so each keeps its own. The
    // same URL's next response differs too, so it depends on something else: refused.
    expect(route.states['{"cookies":["cart"],"bot":""}']).toEqual({ state: "learning" });
    await get("/q/3", "cart=1");
    expect(route.states).toEqual({
      "": { state: "served" },
      '{"cookies":["cart"],"bot":""}': {
        state: "refused",
        reason: expect.stringContaining("two responses for one request differ") as string,
      },
    });
    expect(await get("/q/1234567890123456789012345678901234567890")).toBe("compiled");
    expect(await get("/q/3", "cart=1")).toBeNull();
    server.close();
  });
});

describe("a render's fragments (ADR-0031)", () => {
  const out = {
    html: Buffer.from("<main>é</main>"),
    segments: Buffer.from("<div hidden>s</div>"),
    flight: Buffer.from('["$","main",null,{"children":"$L!1"}]'),
    rows: [Buffer.from('["$","p",null,{"a":"$!0:props"}]')],
  };

  it("are bytes to serve, and strings with the rows' ids to learn from", () => {
    const [page, segments, row] = fragments(out, ["0", "a"]);
    expect([page?.name, segments?.name, row?.name]).toEqual(["page", "segments", "row:1"]);
    expect(page?.html).toBe("<main>é</main>");
    expect(page?.flight).toBe('["$","main",null,{"children":"$La"}]');
    expect(page?.bytes?.html).toBe(out.html);
    expect(page?.bytes?.script.toString()).toBe(
      String.raw`[\"$\",\"main\",null,{\"children\":\"$La\"}]`,
    );
    expect(row?.flight).toBe('["$","p",null,{"a":"$0:props"}]');
    expect(row?.html).toBe("");
    expect(row?.bytes?.script.toString()).toBe(String.raw`[\"$\",\"p\",null,{\"a\":\"$0:props\"}]`);
    // Segments have HTML only.
    expect(segments?.html).toBe("<div hidden>s</div>");
    expect(segments?.flight).toBe("");
    expect(segments?.bytes?.script.length).toBe(0);
  });

  it("keep placeholders without ids, and have no segments when there are none", () => {
    const [page, row] = fragments({ ...out, segments: Buffer.alloc(0) }, undefined);
    expect(row?.name).toBe("row:1");
    expect(page?.flight).toContain('"$L!1"');
    expect(page?.bytes?.script.toString()).toContain(String.raw`\"$L!1\"`);
  });
});

describe("re-check timing (ADR-0032)", () => {
  /** Which of `n` requests are re-checks, the clock at `at(i)` ms for request `i`. */
  function due(rechecks: (now: () => number) => Rechecks, n: number, at: (i: number) => number) {
    let i = 0;
    const r = rechecks(() => at(i));
    const out: number[] = [];
    for (; i < n; i++) if (r.due()) out.push(i);
    return out;
  }

  it("re-checks one request in `every` while requests are slower than the interval", () => {
    // 10 requests a second: every 4th is due, and 400 ms apart is past the 100 ms interval.
    expect(
      due(
        (now) => new Rechecks(4, 100, now),
        12,
        (i) => i * 100,
      ),
    ).toEqual([3, 7, 11]);
  });

  it("re-checks at most once an interval however busy the route is", () => {
    // 1,000 requests a second, every 4th would be due: one a second instead.
    expect(
      due(
        (now) => new Rechecks(4, 1000, now),
        3000,
        (i) => i,
      ),
    ).toEqual([3, 1003, 2003]);
  });

  it("never re-checks with `every` 0, and only counts with interval 0", () => {
    expect(
      due(
        (now) => new Rechecks(0, 0, now),
        50,
        () => 0,
      ),
    ).toEqual([]);
    expect(
      due(
        (now) => new Rechecks(5, 0, now),
        10,
        () => 0,
      ),
    ).toEqual([4, 9]);
  });
});

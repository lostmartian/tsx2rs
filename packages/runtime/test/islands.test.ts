import { readFileSync } from "node:fs";
import { createServer, type IncomingMessage, type Server, type ServerResponse } from "node:http";
import { join } from "node:path";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { Route, type CompiledRoute, type Renderer } from "../src/compiled.js";
import type { Islands } from "../src/encode.js";
import { IslandCache, Miss, learnPage, placeholders } from "../src/islands.js";

/**
 * Real Next.js 16.4.5 documents for `app/p/[n]/page.tsx`, whose `<Counter />` is a client
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
  it("answers islands and boundaries with markers and references of their own, in order", () => {
    const { islands, boundaries, calls } = placeholders();
    expect(islands(3, { href: "/" })).toEqual({
      ref: "$L0",
      before: "<!--tsx2rs:0:b-->",
      after: "<!--tsx2rs:0:a-->",
    });
    // A boundary is pending: its fallback in the shell, its content in the segments.
    expect(boundaries()).toEqual({
      ref: "$L1",
      pending: true,
      open: "<!--tsx2rs:1:o-->",
      close: "<!--tsx2rs:1:c-->",
      segment: "<!--tsx2rs:1:s-->",
      end: "<!--tsx2rs:1:e-->",
    });
    expect(islands(3, { href: "/x" }).ref).toBe("$L2");
    expect(calls).toEqual([
      { kind: "island", index: 3, props: '{"href":"/"}' },
      { kind: "boundary" },
      { kind: "island", index: 3, props: '{"href":"/x"}' },
    ]);
  });
});

/** A document with `html`, then a Flight script with `row` as row 2. */
const documentWith = (html: string, row: string) =>
  `<body>${html}</body><script>self.__next_f.push(${JSON.stringify([1, `2:${row}\n`])})</script>`;

describe("learnPage", () => {
  it("learns an island's HTML and reference from Next.js's response", () => {
    const { islands, calls } = placeholders();
    const learned = learnPage(doc("5"), { ...page("5", islands), segments: "" }, calls);
    // The page as Next.js wrote it: what the wrapper is then captured around.
    const { html, flight } = page("5", () => COUNTER);
    expect(learned).toEqual({ ok: true, parts: [COUNTER], html, segments: "", flight });
    expect(doc("5")).toContain(learned.ok ? learned.html : "");
  });

  it("learns the HTML on both sides of an island's children", () => {
    const flight = (ref: string) =>
      `["$","nav",null,{"children":[["$","${ref}",null,{"href":"/x","children":["go ",["$","b",null,{"children":"9"}]]}],"!"]}]`;
    const { islands, calls } = placeholders();
    const link = islands(0, { href: "/x" });
    const html = `<nav>${link.before}go <b>9</b>${link.after}!</nav>`;
    const document = documentWith('<nav><a href="/x">go <b>9</b></a>!</nav>', flight("$L1f"));
    expect(
      learnPage(document, { html, segments: "", flight: flight(link.ref) }, calls),
    ).toMatchObject({
      ok: true,
      parts: [{ ref: "$L1f", before: '<a href="/x">', after: "</a>" }],
      html: '<nav><a href="/x">go <b>9</b></a>!</nav>',
    });
  });

  /** A page with one boundary, rendered with placeholders. */
  function boundaryPage() {
    const { boundaries, islands, calls } = placeholders();
    const b = boundaries();
    // Its content is a client component (`<a>` in Next.js's HTML).
    const link = islands(0, { href: "/" });
    return {
      calls,
      flight: (ref: string, linkRef: string) =>
        `["$","main",null,{"children":[["$","${ref}",null,{"fallback":["$","i",null,{"children":"wait"}],"children":["$","${linkRef}",null,{"href":"/"}]}],"!"]}]`,
      page: {
        html: `<main>${b.open}<i>wait</i>${b.close}!</main>`,
        segments: `${b.segment}${link.before}${b.end}`,
      },
      refs: [b.ref, link.ref] as const,
    };
  }

  it("learns a resolved boundary: its content inline, no fallback", () => {
    const { calls, flight, page, refs } = boundaryPage();
    const document = documentWith(
      '<main><!--$--><a href="/">x</a><!--/$-->!</main>',
      flight("$b", "$L7"),
    );
    expect(learnPage(document, { ...page, flight: flight(...refs) }, calls)).toEqual({
      ok: true,
      parts: [
        { ref: "$b", pending: false, open: "<!--$-->", close: "", segment: "", end: "<!--/$-->" },
        { ref: "$L7", before: '<a href="/">x</a>', after: "" },
      ],
      html: '<main><!--$--><a href="/">x</a><!--/$-->!</main>',
      segments: "",
      flight: flight("$b", "$L7"),
    });
  });

  it("learns a pending boundary: its fallback in the shell, its content in a segment", () => {
    const { calls, flight, page, refs } = boundaryPage();
    const segment = '<div hidden id="S:3"><a href="/">x</a></div><script>$RC("B:2","S:3")</script>';
    const document = documentWith(
      `<main><!--$?--><template id="B:2"></template><i>wait</i><!--/$-->!</main>${segment}`,
      flight("$b", "$L7"),
    );
    expect(learnPage(document, { ...page, flight: flight(...refs) }, calls)).toEqual({
      ok: true,
      parts: [
        {
          ref: "$b",
          pending: true,
          open: '<!--$?--><template id="B:2"></template>',
          close: "<!--/$-->",
          segment: '<div hidden id="S:3">',
          end: '</div><script>$RC("B:2","S:3")</script>',
        },
        { ref: "$L7", before: '<a href="/">x</a>', after: "" },
      ],
      html: '<main><!--$?--><template id="B:2"></template><i>wait</i><!--/$-->!</main>',
      segments: segment,
      flight: flight("$b", "$L7"),
    });
  });

  it("refuses what it can't place exactly once", () => {
    const { islands, calls } = placeholders();
    const fragment = { ...page("5", islands), segments: "" };
    // Another page's response: the page's own text isn't there.
    expect(learnPage(doc("6"), fragment, calls)).toEqual({
      ok: false,
      reason:
        "the page with its islands and boundaries found 0 times in Next.js's response, not once",
    });
    expect(learnPage(doc("5"), fragment, [...calls, ...calls])).toMatchObject({
      ok: false,
      reason: "the page's Flight references aren't the islands and boundaries it rendered",
    });
    expect(
      learnPage(doc("5"), { ...fragment, flight: fragment.flight.replace("Product", "P") }, calls),
    ).toMatchObject({
      ok: false,
      reason:
        "the page's Flight row with its islands found 0 times in Next.js's response, not once",
    });
    expect(learnPage(doc("5"), { ...fragment, html: "<!--tsx2rs:0:b-->" }, calls)).toMatchObject({
      ok: false,
      reason: "the page starts with an island or a boundary",
    });
    // A boundary in neither of React's forms.
    const { calls: bc, flight, page: bp, refs } = boundaryPage();
    const odd = documentWith(
      '<main><!--x--><a href="/">x</a><!--/$-->!</main>',
      flight("$b", "$L7"),
    );
    expect(learnPage(odd, { ...bp, flight: flight(...refs) }, bc)).toMatchObject({ ok: false });
  });
});

describe("learnPage, at the edges", () => {
  /** One boundary with `fallback` and `content` in the placeholders' form, around `before`. */
  function render(
    build: (p: ReturnType<typeof placeholders>) => { html: string; segments: string },
  ) {
    const p = placeholders();
    const page = build(p);
    // A Flight row with one reference per call, in order.
    const flight = (refs: string[]) => `[${refs.map((r) => `"${r}"`).join(",")}]`;
    const ours = flight(p.calls.map((_, n) => `$L${n.toString(16)}`));
    return {
      learn: (html: string, refs: string[]) =>
        learnPage(documentWith(html, flight(refs)), { ...page, flight: ours }, p.calls),
    };
  }

  it("gives an island in a resolved boundary's fallback its reference and no HTML", () => {
    const { learn } = render((p) => {
      const b = p.boundaries();
      const icon = p.islands(0, {});
      return {
        html: `<main>${b.open}<i>${icon.before}</i>${b.close}</main>`,
        segments: `${b.segment}<b>x</b>${b.end}`,
      };
    });
    expect(learn("<main><!--$--><b>x</b><!--/$--></main>", ["$b", "$L3"])).toMatchObject({
      ok: true,
      parts: [{ pending: false }, { ref: "$L3", before: "", after: "" }],
    });
  });

  it("ends an island before a boundary where React's marker starts", () => {
    const { learn } = render((p) => {
      const link = p.islands(0, {});
      const b = p.boundaries();
      return {
        html: `<main>${link.before}${b.open}${b.close}</main>`,
        segments: `${b.segment}<b>x</b>${b.end}`,
      };
    });
    expect(learn("<main><a>l</a><!--$--><b>x</b><!--/$--></main>", ["$L3", "$b"])).toMatchObject({
      ok: true,
      parts: [{ before: "<a>l</a>" }, { pending: false }],
    });
  });

  it("refuses what React's markers don't explain", () => {
    const one = render((p) => {
      const b = p.boundaries();
      return {
        html: `<main>${b.open}<i>f</i>${b.close}</main>`,
        segments: `${b.segment}<b>x</b>${b.end}`,
      };
    });
    const refused = (html: string) => one.learn(html, ["$b"]).ok;
    // Resolved, but not closed; pending, but not closed.
    expect(refused("<main><!--$--><b>x</b></main>")).toBe(false);
    expect(refused('<main><!--$?--><template id="B:0"></template><i>f</i></main>')).toBe(false);
    // Pending, without its segment, without its reveal script, or with another one after it.
    const shell = '<main><!--$?--><template id="B:0"></template><i>f</i><!--/$--></main>';
    expect(refused(shell)).toBe(false);
    expect(refused(`${shell}<script>$RC("B:0","S:0")</script>`)).toBe(false);
    expect(refused(`${shell}<div hidden id="S:0"><b>x</b></div><p></p>$RC("B:0","S:0")`)).toBe(
      false,
    );
    expect(
      refused(`${shell}<div hidden id="S:0"><b>y</b></div><script>$RC("B:0","S:0")</script>`),
    ).toBe(false);
    // Two islands with nothing between them can't be told apart.
    const twice = render((p) => {
      const [a, b] = [p.islands(0, {}), p.islands(0, {})];
      return { html: `<main>${a.before}${b.before}</main>`, segments: "" };
    });
    expect(twice.learn("<main><a></a><a></a></main>", ["$L1", "$L1"])).toMatchObject({
      ok: false,
      reason:
        "the page with its islands and boundaries found 0 times in Next.js's response, not once",
    });
  });

  it("refuses a page whose segments aren't one run in the document", () => {
    const two = render((p) => {
      const [a, b] = [p.boundaries(), p.boundaries()];
      return {
        html: `<main>${a.open}<i>1</i>${a.close}${b.open}<i>2</i>${b.close}</main>`,
        segments: `${a.segment}<b>1</b>${a.end}${b.segment}<b>2</b>${b.end}`,
      };
    });
    const pending = (n: number) =>
      `<!--$?--><template id="B:${String(n)}"></template><i>${String(n)}</i><!--/$-->`;
    const segment = (n: number) =>
      `<div hidden id="S:${String(n)}"><b>${String(n)}</b></div><script>$RC("B:${String(n)}","S:${String(n)}")</script>`;
    const shell = `<main>${pending(1)}${pending(2)}</main>`;
    expect(two.learn(`${shell}${segment(1)}${segment(2)}`, ["$b", "$b"])).toMatchObject({
      ok: true,
      segments: `${segment(1)}${segment(2)}`,
    });
    expect(
      two.learn(`${shell}${segment(1)}<p>layout</p>${segment(2)}`, ["$b", "$b"]),
    ).toMatchObject({
      ok: false,
    });
  });
});

describe("IslandCache", () => {
  it("answers a request with what it learned for it, in order, and misses anything else", () => {
    const cache = new IslandCache();
    const boundary = { ref: "$b", pending: false, open: "", close: "", segment: "", end: "" };
    cache.set(
      "/p/5",
      [{ kind: "island", index: 0, props: "{}" }, { kind: "boundary" }],
      [COUNTER, boundary],
    );
    const hit = cache.resolver("/p/5");
    expect(hit.islands(0, {})).toBe(COUNTER);
    expect(hit.boundaries()).toBe(boundary);
    // Another request, other props, another component, a boundary where an island was.
    expect(() => cache.resolver("/p/6").islands(0, {})).toThrow(Miss);
    expect(() => cache.resolver("/p/5").islands(0, { a: 1 })).toThrow(Miss);
    expect(() => cache.resolver("/p/5").islands(1, {})).toThrow(Miss);
    expect(() => cache.resolver("/p/5").boundaries()).toThrow(Miss);
  });

  it("keeps the last 10,000 requests", () => {
    const cache = new IslandCache();
    for (let i = 0; i <= 10_000; i++) cache.set(`/r/${String(i)}`, [], []);
    expect(() => cache.resolver("/r/0").boundaries()).toThrow(Miss);
    cache.set(
      "/r/1",
      [{ kind: "boundary" }],
      [{ ref: "$b", pending: false, open: "", close: "", segment: "", end: "" }],
    );
    expect(cache.resolver("/r/1").boundaries().ref).toBe("$b");
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
  const renderer: Renderer = async (request, parts) => {
    if (!parts) throw new Error("no resolver");
    return [page(request.params["n"] ?? "", parts.islands)];
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

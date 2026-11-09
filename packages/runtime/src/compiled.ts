/**
 * Serving a compiled route (milestone 1, step 8.3; ADR-0018, ADR-0019).
 *
 * A compiled route starts out **learning**: its requests go to Next.js, and the server keeps a
 * copy of Next.js's response. For the same request it runs the route itself (the page's data
 * function, the generated encoder, the engine) and gives both to `capture`:
 * - from the first response it **learns** the wrapper;
 * - from a response for other params it **confirms** it.
 *
 * From then on the route is **served**: data function, encoder, engine, `fill`, with Next.js's
 * status and headers. Anything that goes wrong on tsx2rs's side **refuses** the route, which
 * Next.js then serves, with the reason recorded:
 * - a value the engine can't take natively (`NotNative`);
 * - a data function that throws (one that reads Next.js's request context, for now);
 * - a response that differs in ways the slots don't explain.
 *
 * Served routes are **re-checked** (ADR-0027, ADR-0032): once at least `verifyEvery` requests it
 * would serve have passed and at least `verifyInterval` ms since its last re-check, Next.js answers
 * instead, and the route compares its own document with Next.js's. A difference
 * means what it learned has gone stale (a product's title changed, a menu got another item): it
 * forgets what it learned for that request, or relearns its shared wrapper.
 *
 * A page's metadata is a fragment of its request too, learned from Next.js's response for that
 * request (ADR-0022). A page with client components renders their islands from what it learned for
 * the same request (ADR-0020). A request it hasn't learned them for goes to Next.js, rendered with placeholders,
 * and the runtime learns them from Next.js's response before it learns or confirms the wrapper.
 */
import type { IncomingMessage, ServerResponse } from "node:http";
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { join } from "node:path";
import { Duplex } from "node:stream";
import { brotliDecompressSync, gunzipSync, inflateSync } from "node:zlib";
import { pathToFileURL } from "node:url";
import { Program, flightScripts } from "@tsx2rs/native";
import {
  confirm,
  confirmed,
  fill,
  fragmentNames,
  learn,
  type Fragment,
  type Template,
} from "./capture.ts";
import type { Boundaries, Islands } from "./encode.ts";
import {
  IslandCache,
  Miss,
  learnPage,
  placeholders,
  withIds,
  type Call,
  type Resolvers,
} from "./islands.ts";
import { MetadataCache, findMetadata } from "./metadata.ts";

/** A compiled page route in the manifest. */
export interface CompiledRoute {
  page: string;
  pathnames: string[];
  entrypoint: string;
  program: string;
  encoder: string;
  entry: DataFunctionName;
  callees: DataFunctionName[];
}

interface DataFunctionName {
  module: string;
  name: string;
}

type DataFunction = (props: unknown) => unknown;
type Sample = Parameters<typeof learn>[0];
type Encode = (
  record: unknown,
  data: DataFunction[],
  islands?: Islands,
  boundaries?: Boundaries,
) => Uint8Array | Promise<Uint8Array>;

/** Where a compiled route stands. */
export type RouteState =
  | { state: "learning" }
  | { state: "learned"; params: Record<string, string> }
  /** `perRequest`: each URL is served with the wrapper learned from its own response (ADR-0023). */
  | { state: "served"; perRequest?: true }
  | { state: "refused"; reason: string };

/** What a request asks a compiled route for. */
export interface Request {
  /** Its variant: what else Next.js's wrapper may depend on (`variant` in `server.ts`). */
  variant?: string;
  params: Record<string, string>;
  pathname: string;
  search: string;
  query: Record<string, string | string[]>;
}

/** Next.js's status and headers for the route's documents. */
interface Head {
  status: number;
  headers: Record<string, string | string[]>;
}

/**
 * Headers that belong to one response's transfer, not to the document (served pages are
 * compressed per request, as `next start` compresses them).
 */
const TRANSFER = new Set([
  "content-encoding",
  "transfer-encoding",
  "content-length",
  "date",
  "connection",
  "keep-alive",
]);

/** How many URLs' wrappers a variant keeps when they're per request (the oldest go first). */
const REQUESTS = 1_000;

/** The registry the adapter's loader fills (ADR-0017). */
const REGISTRY = Symbol.for("tsx2rs.data");

/**
 * What a route has learned for one **variant** of its requests: requests that differ in what
 * Next.js's wrapper may depend on besides params and the URL (which cookies are set, whether the
 * client is a bot). Each variant learns, confirms and is refused on its own, so a visitor with a
 * cart doesn't stop tsx2rs serving visitors without one.
 */
class Variant {
  status: RouteState = { state: "learning" };
  template: Template | undefined;
  head: Head | undefined;
  /**
   * The response the template was learned from, with its metadata, until the template is
   * confirmed: relearned with the metadata as a fragment if the wrapper can't be confirmed with
   * the metadata in it (ADR-0022).
   */
  private first: { sample: Sample; metadata: Fragment } | undefined;
  /** The URL the template was learned from (`Route.handle`'s request key). */
  private learnedFrom = "";
  /**
   * Per-request wrappers (ADR-0023), by URL, once the wrapper turns out to depend on the request
   * in ways the params and fragments don't explain (a layout that marks the current page's link).
   */
  private requests: Map<string, Template> | undefined;
  /** Whether one URL's second response matched its first: then a URL's first is enough. */
  private repeatable = false;

  /** The template to fill for the request `url`, with `params`, if there is one. */
  templateFor(url: string, params: Record<string, string>): Template | undefined {
    if (this.requests) return this.repeatable ? this.requests.get(url) : undefined;
    return this.template && this.canFill(params) ? this.template : undefined;
  }

  canFill(params: Record<string, string>): boolean {
    const status = this.status;
    if (status.state === "served") return true;
    return (
      status.state === "learned" &&
      Object.entries(params).every(([name, value]) => status.params[name] === value)
    );
  }

  /**
   * Learns from, or confirms with, a response Next.js served. The page's `metadata`, if it has any,
   * stays part of the wrapper while the params explain it, so any request can be served; only a
   * wrapper that can't be confirmed otherwise takes it per request.
   */
  study(sample: Sample, response: Head, metadata: Fragment | undefined, url: string): void {
    const withMetadata = (s: Sample, m: Fragment): Sample => ({
      ...s,
      fragments: [...s.fragments, m],
    });
    if (this.requests) {
      this.studyRequest(sample, response, url);
      return;
    }
    if (!this.template) {
      const learned = learn(sample);
      if (!learned.ok) {
        this.refuse(learned.reason);
        return;
      }
      this.template = learned.template;
      this.head = { status: response.status, headers: response.headers };
      this.status = { state: "learned", params: sample.params };
      this.first = metadata && { sample, metadata };
      this.learnedFrom = url;
      return;
    }
    if (JSON.stringify(response.headers) !== JSON.stringify(this.head?.headers)) {
      this.refuse("Next.js's headers differ between requests");
      return;
    }
    const perRequest = fragmentNames(this.template).has("metadata");
    let result = confirm(
      this.template,
      perRequest && metadata ? withMetadata(sample, metadata) : sample,
    );
    if (!result.ok && !perRequest && this.first && metadata) {
      const relearned = learn(withMetadata(this.first.sample, this.first.metadata));
      if (relearned.ok) {
        const again = confirm(relearned.template, withMetadata(sample, metadata));
        if (again.ok) result = again;
      }
    }
    if (!result.ok) {
      // The wrapper depends on the request: each URL keeps its own (ADR-0023).
      const own = learn(sample);
      if (own.ok) {
        this.requests = new Map([
          [this.learnedFrom, this.template],
          [url, own.template],
        ]);
        this.template = undefined;
        this.first = undefined;
        this.status = { state: "learning" };
        return;
      }
      this.refuse(result.reason);
      return;
    }
    this.template = result.template;
    if (confirmed(result.template)) {
      this.status = { state: "served" };
      this.first = undefined;
    }
  }

  /**
   * Per-request wrappers: a URL's first response teaches its wrapper; a second response for a URL
   * must match it, and once one has, a URL's first response is enough.
   */
  private studyRequest(sample: Sample, response: Head, url: string): void {
    const requests = this.requests;
    if (!requests) return;
    if (JSON.stringify(response.headers) !== JSON.stringify(this.head?.headers)) {
      this.refuse("Next.js's headers differ between requests");
      return;
    }
    const known = requests.get(url);
    if (known) {
      const again = confirm(known, sample);
      if (!again.ok) {
        this.refuse(`two responses for one request differ: ${again.reason}`);
        return;
      }
      requests.set(url, again.template);
      this.repeatable = true;
      this.status = { state: "served", perRequest: true };
      return;
    }
    const own = learn(sample);
    if (!own.ok) {
      this.refuse(own.reason);
      return;
    }
    requests.set(url, own.template);
    if (requests.size > REQUESTS) {
      const oldest = requests.keys().next();
      if (oldest.done !== true) requests.delete(oldest.value);
    }
  }

  /**
   * Forgets what was learned for the request `url`, after Next.js answered it differently: its own
   * wrapper, or the shared one (which the next responses relearn and confirm).
   */
  forget(url: string): void {
    if (this.requests) {
      this.requests.delete(url);
      return;
    }
    this.template = undefined;
    this.head = undefined;
    this.first = undefined;
    this.status = { state: "learning" };
  }

  refuse(reason: string): void {
    this.status = { state: "refused", reason };
    this.template = undefined;
    this.first = undefined;
    this.requests = undefined;
  }
}

/** A compiled route: what it needs to run, and what each variant of its requests has learned. */
export class Route {
  readonly route: CompiledRoute;
  /** Set when tsx2rs can't render the route at all: every variant is Next.js's. */
  private refused: string | undefined;
  private readonly variants = new Map<string, Variant>();
  private readonly islands = new IslandCache();
  private readonly metadata = new MetadataCache();
  private readonly render: Renderer;
  /** Next.js's `compression` middleware, as `next start` applies it (unless `compress: false`). */
  private readonly compress: Compress | undefined;

  /** When Next.js answers a request the route would serve, and the route compares. */
  private readonly rechecks: Rechecks;
  /** Re-checks so far, and how many found a difference. */
  readonly checks = { compared: 0, differed: 0 };

  /**
   * A route that re-checks after `verifyEvery` requests it would serve (0: never), at most once
   * every `verifyInterval` ms.
   */
  constructor(
    route: CompiledRoute,
    render: Renderer,
    compress?: Compress,
    verifyEvery = 0,
    verifyInterval = 0,
  ) {
    this.route = route;
    this.render = render;
    this.compress = compress;
    this.rechecks = new Rechecks(verifyEvery, verifyInterval);
  }

  /** The state of the default variant (requests without cookies, not from a bot), or the
   * route's refusal. */
  get state(): RouteState {
    if (this.refused !== undefined) return { state: "refused", reason: this.refused };
    return this.variants.get("")?.status ?? { state: "learning" };
  }

  /** Every variant's state, by variant key. */
  get states(): Record<string, RouteState> {
    const out: Record<string, RouteState> = {};
    for (const [key, variant] of this.variants) out[key] = variant.status;
    return out;
  }

  /** Serves `request`, or hands it to `next` (and learns from Next.js's response). */
  async handle(
    req: IncomingMessage,
    res: ServerResponse,
    request: Request,
    next: (req: IncomingMessage, res: ServerResponse) => Promise<void>,
  ): Promise<void> {
    let variant = this.variants.get(request.variant ?? "");
    if (!variant) {
      variant = new Variant();
      this.variants.set(request.variant ?? "", variant);
    }
    if (this.refused !== undefined || variant.status.state === "refused") {
      await next(req, res);
      return;
    }
    // The islands learned for this request, or placeholders to learn them with.
    const url = `${request.variant ?? ""}\0${request.pathname}${request.search}`;
    let pending: Call[] | undefined;
    const fragments = await this.render(request, this.islands.resolver(url))
      .catch((error: unknown) => {
        if (!(error instanceof Miss)) throw error;
        const learning = placeholders();
        pending = learning.calls;
        return this.render(request, learning);
      })
      .catch((error: unknown) => {
        // `notFound()`, `redirect()`: this request's answer is Next.js's, not the route's.
        if (controlFlow(error)) return null;
        this.refused = `tsx2rs can't render it: ${error instanceof Error ? error.message : String(error)}`;
        return undefined;
      });
    if (fragments === null) {
      await next(req, res);
      return;
    }
    if (fragments === undefined) {
      await next(req, res);
      return;
    }
    // The metadata learned for this request, if the wrapper is filled with it (ADR-0022).
    const metadata = this.metadata.get(url);
    const values = {
      params: request.params,
      pathname: request.pathname,
      search: request.search,
      fragments: Object.fromEntries(
        [...fragments, ...(metadata ? [metadata] : [])].map((f) => [f.name, f]),
      ),
    };
    const template = variant.templateFor(url, request.params);
    if (
      pending === undefined &&
      template &&
      variant.head &&
      (metadata || !fragmentNames(template).has("metadata"))
    ) {
      const body = fill(template, values);
      if (req.method !== "HEAD" && this.rechecks.due()) {
        // A re-check: Next.js answers, and what this route would have sent must be the same.
        await tee(req, res, next, (response) => {
          if (response.status !== variant.head?.status) return;
          this.checks.compared++;
          if (body.equals(Buffer.from(response.body))) return;
          this.checks.differed++;
          this.islands.forget(url);
          this.metadata.forget(url);
          variant.forget(url);
        });
        return;
      }
      // Next.js's middleware compresses only for a client that accepts an encoding.
      const encodings = req.headers["accept-encoding"] ?? "";
      if (this.compress && /gzip|br|deflate|\*/.test(encodings)) {
        this.compress(req, res, () => undefined);
        // On a client disconnect the middleware never ends its zlib stream (Next.js's
        // `releaseCompressionStream`).
        res.once("close", () => {
          if (!res.writableFinished) release(res);
        });
      }
      res.writeHead(variant.head.status, { ...variant.head.headers, "x-tsx2rs": "compiled" });
      res.end(req.method === "HEAD" ? undefined : body);
      return;
    }
    const learning = variant;
    const calls = pending;
    await tee(req, res, next, (response) => {
      const type = response.headers["content-type"];
      const html = typeof type === "string" && type.startsWith("text/html");
      if (response.status !== 200 || !html) return;
      let learned = fragments;
      if (calls !== undefined) {
        const page = fragments.find((f) => f.name === "page");
        const segments = fragments.find((f) => f.name === "segments")?.html ?? "";
        const rows = fragments.filter((f) => f.name.startsWith("row:")).map((f) => f.flight);
        const parts = page && learnPage(response.body, { ...page, segments, rows }, calls);
        if (!parts?.ok) {
          learning.refuse(parts?.reason ?? "no page fragment");
          return;
        }
        this.islands.set(url, calls, parts.parts, parts.ids);
        learned = [
          { name: "page", html: parts.html, flight: parts.flight },
          ...(parts.segments === ""
            ? []
            : [{ name: "segments", html: parts.segments, flight: "" }]),
          ...parts.rows.map((row, i) => ({ name: `row:${String(i + 1)}`, html: "", flight: row })),
        ];
      }
      const found = findMetadata(response.body);
      if (typeof found === "string") {
        learning.refuse(found);
        return;
      }
      this.metadata.set(url, found ?? null);
      learning.study(
        { document: response.body, ...values, fragments: learned },
        response,
        found,
        url,
      );
    });
  }
}

/**
 * When a route re-checks (ADR-0027, ADR-0032): once at least `every` requests it would serve have
 * passed since the last re-check, and at least `interval` ms. The count keeps re-checks to at most
 * one request in `every`; the time keeps them to at most one a second (by default) however busy the
 * route is, as each costs a whole Next.js render.
 */
export class Rechecks {
  private readonly every: number;
  private readonly interval: number;
  private readonly now: () => number;
  private since = 0;
  private last = Number.NEGATIVE_INFINITY;

  /** `every` 0 never re-checks. `now` is the clock, in ms. */
  constructor(every: number, interval: number, now: () => number = () => performance.now()) {
    this.every = every;
    this.interval = interval;
    this.now = now;
  }

  /** Whether the request the route is about to serve is re-checked instead. */
  due(): boolean {
    if (this.every <= 0 || ++this.since < this.every) return false;
    const now = this.now();
    if (now - this.last < this.interval) return false;
    this.since = 0;
    this.last = now;
    return true;
  }
}

/**
 * What a route renders per request: its fragments (ADR-0018), the page and its segments (the
 * content of pending `<Suspense>` boundaries, ADR-0024), with what React renders inside it.
 */
export type Renderer = (request: Request, parts?: Resolvers) => Promise<Fragment[]>;

/**
 * The renderer of a compiled route of the app in `dir`: the page's data function (from the
 * server bundle's registry, ADR-0017), the generated encoder, the engine.
 */
export function renderer(route: CompiledRoute, dir: string, manifestDir: string): Renderer {
  let loaded:
    | Promise<{ program: Program; encode: Encode; entry: DataFunction; callees: DataFunction[] }>
    | undefined;
  const load = () => {
    loaded ??= (async () => {
      const app = createRequire(join(dir, "package.json"));
      // Evaluating the route's modules registers their data functions (ADR-0017).
      const mod = app(join(dir, route.entrypoint)) as {
        routeModule: { userland: { loaderTree: unknown } };
      };
      await evaluate(mod.routeModule.userland.loaderTree);
      const registry = (
        globalThis as Record<symbol, Map<string, Record<string, DataFunction>> | undefined>
      )[REGISTRY];
      const fn = (f: DataFunctionName): DataFunction => {
        const found = registry?.get(f.module)?.[f.name];
        if (!found) throw new Error(`${f.module}: ${f.name} isn't in the server bundle`);
        return found;
      };
      const encoder = (await import(pathToFileURL(join(manifestDir, route.encoder)).href)) as {
        encode: Encode;
      };
      return {
        program: new Program(readFileSync(join(manifestDir, route.program))),
        encode: encoder.encode,
        entry: fn(route.entry),
        callees: route.callees.map(fn),
      };
    })();
    return loaded;
  };
  return async (request, parts) => {
    const { program, encode, entry, callees } = await load();
    const props = {
      params: Promise.resolve(request.params),
      searchParams: Promise.resolve(request.query),
    };
    const record = await entry(props);
    // Async when the page has an async component (ADR-0025).
    const input = await encode(record, callees, parts?.islands, parts?.boundaries);
    const out = program.render(Buffer.from(input.buffer, input.byteOffset, input.byteLength));
    if (out.head !== "" || out.hints !== "") {
      throw new Error("the page hoists into <head> (not yet served by tsx2rs)");
    }
    // The page's row, then the rows it defers to (ADR-0025), with the ids Next.js gave them, or
    // placeholders while they're learned. References to rows or to objects written before need
    // the rows' ids (ADR-0025, ADR-0026).
    const rows = [out.flight, ...out.rows];
    const placeholders = rows.some((row) => row.includes('"$L!') || row.includes('"$!'));
    if (placeholders && !parts) throw new Error("the page's Flight rows need their ids");
    const ids = placeholders ? parts?.rows(out.rows.length) : undefined;
    return fragments(out, ids);
  };
}

const EMPTY = Buffer.alloc(0);

/** What the engine renders for a page, as bytes (the napi `Rendered`). */
export interface RenderedBytes {
  html: Buffer;
  segments: Buffer;
  flight: Buffer;
  rows: Buffer[];
}

/**
 * A render's fragments: the page, its segments if any, and the rows it defers to, with `ids` put
 * in their placeholders (or kept, without). Served as bytes, the rows escaped for their scripts
 * by the engine the first time a response needs them; as strings only when read, for learning
 * and re-checks (ADR-0031).
 */
export function fragments(out: RenderedBytes, ids: string[] | undefined): Fragment[] {
  const rows = [out.flight, ...out.rows];
  let scripts: Buffer[] | undefined;
  const script = (row: number) => (scripts ??= flightScripts(rows, ids))[row] ?? EMPTY;
  const text = (row: number) => {
    const value = rows[row]?.toString() ?? "";
    return ids === undefined ? value : withIds(value, ids);
  };
  const fragment = (name: string, html: Buffer, row?: number): Fragment => {
    let htmlText: string | undefined;
    let flightText: string | undefined;
    return {
      name,
      get html() {
        return (htmlText ??= html.toString());
      },
      get flight() {
        return row === undefined ? "" : (flightText ??= text(row));
      },
      get bytes() {
        return { html, script: row === undefined ? EMPTY : script(row) };
      },
    };
  };
  return [
    fragment("page", out.html, 0),
    ...(out.segments.length === 0 ? [] : [fragment("segments", out.segments)]),
    ...out.rows.map((_, i) => fragment(`row:${String(i + 1)}`, EMPTY, i + 1)),
  ];
}

/** Next.js's `compression` middleware for the app in `dir`, as `next start` applies it. */
export function compression(dir: string): Compress {
  const app = createRequire(join(dir, "package.json"));
  return (app("next/dist/compiled/compression") as () => Compress)();
}

export type Compress = (req: IncomingMessage, res: ServerResponse, next: () => void) => void;

/** Next.js's `releaseCompressionStream`: destroys the zlib stream `compression` put on `res`. */
function release(res: ServerResponse): void {
  const noop = () => undefined;
  const stream = res.on("drain", noop) as unknown;
  if (stream === res || !(stream instanceof Duplex)) {
    res.off("drain", noop);
    return;
  }
  stream.destroy();
}

/** A response body as sent with `encoding`, decompressed; `undefined` for an unknown encoding. */
function decompress(body: Buffer, encoding: string | string[] | undefined): Buffer | undefined {
  switch (encoding) {
    case undefined:
    case "identity":
      return body;
    case "gzip":
      return gunzipSync(body);
    case "deflate":
      return inflateSync(body);
    case "br":
      return brotliDecompressSync(body);
    default:
      return undefined;
  }
}

/**
 * Whether `error` is Next.js's control flow rather than a failure: `notFound()`, `forbidden()`,
 * `unauthorized()` (`NEXT_HTTP_ERROR_FALLBACK;<status>`) or `redirect()` (`NEXT_REDIRECT;…`), as
 * Next.js 16.4.5 marks them (`isHTTPAccessFallbackError`, `isRedirectError`).
 */
function controlFlow(error: unknown): boolean {
  if (typeof error !== "object" || error === null || !("digest" in error)) return false;
  const digest = error.digest;
  return (
    typeof digest === "string" &&
    (digest.startsWith("NEXT_HTTP_ERROR_FALLBACK;") || digest.startsWith("NEXT_REDIRECT;"))
  );
}

/** Calls every segment getter in Next.js's loader tree: that evaluates the route's modules. */
async function evaluate(node: unknown): Promise<void> {
  if (Array.isArray(node) && typeof node[0] === "function" && typeof node[1] === "string") {
    await (node[0] as () => Promise<unknown>)();
    return;
  }
  if (node !== null && typeof node === "object") {
    for (const value of Object.values(node)) await evaluate(value);
  }
}

/**
 * Lets Next.js answer, keeping a copy of what it writes, and gives the copy to `done` before the
 * last bytes go out: by the time the client has the response, the route has learned from it.
 */
async function tee(
  req: IncomingMessage,
  res: ServerResponse,
  next: (req: IncomingMessage, res: ServerResponse) => Promise<void>,
  done: (response: Head & { body: string }) => void,
): Promise<void> {
  const chunks: Buffer[] = [];
  const keep = (chunk: unknown, encoding?: unknown) => {
    if (chunk === undefined || chunk === null || typeof chunk === "function") return;
    chunks.push(
      Buffer.isBuffer(chunk)
        ? chunk
        : chunk instanceof Uint8Array
          ? Buffer.from(chunk)
          : typeof chunk === "string"
            ? Buffer.from(
                chunk,
                typeof encoding === "string" ? (encoding as BufferEncoding) : "utf8",
              )
            : Buffer.alloc(0),
    );
  };
  const write = res.write.bind(res) as (...args: unknown[]) => boolean;
  const end = res.end.bind(res) as (...args: unknown[]) => ServerResponse;
  const writeHead = res.writeHead.bind(res) as (...args: unknown[]) => ServerResponse;
  // Headers given to `writeHead` itself aren't in `getHeaders()`: keep them too.
  const given: Record<string, string | string[]> = {};
  res.writeHead = (status: number, ...rest: unknown[]) => {
    const last = rest.at(-1);
    if (last !== null && typeof last === "object" && !Array.isArray(last)) {
      for (const [name, value] of Object.entries(last)) {
        if (typeof value === "string" || typeof value === "number")
          given[name.toLowerCase()] = String(value);
        else if (Array.isArray(value)) given[name.toLowerCase()] = value.map((v) => String(v));
      }
    }
    return writeHead(status, ...rest);
  };
  res.write = ((chunk: unknown, ...rest: unknown[]) => {
    keep(chunk, rest[0]);
    return write(chunk, ...rest);
  }) as typeof res.write;
  res.end = ((chunk?: unknown, ...rest: unknown[]) => {
    keep(chunk, rest[0]);
    const headers: Record<string, string | string[]> = {};
    for (const [name, value] of Object.entries({ ...res.getHeaders(), ...given })) {
      if (value !== undefined && !TRANSFER.has(name))
        headers[name] = typeof value === "number" ? String(value) : value;
    }
    try {
      // Next.js compressed what the client asked it to: the copy is the document decompressed.
      const encoding = given["content-encoding"] ?? res.getHeader("content-encoding");
      const body = decompress(
        Buffer.concat(chunks),
        typeof encoding === "number" ? String(encoding) : encoding,
      );
      if (body !== undefined)
        done({ status: res.statusCode, headers, body: body.toString("utf8") });
    } finally {
      end(chunk, ...rest);
    }
    return res;
  }) as typeof res.end;
  await next(req, res);
}

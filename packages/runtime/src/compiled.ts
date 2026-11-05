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
 * - a value the engine can't take natively (`NotNative`: a client component, for now);
 * - a data function that throws (one that reads Next.js's request context, for now);
 * - a response that differs in ways the slots don't explain.
 */
import type { IncomingMessage, ServerResponse } from "node:http";
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { join } from "node:path";
import { Duplex } from "node:stream";
import { pathToFileURL } from "node:url";
import { Program } from "@tsx2rs/native";
import { confirm, confirmed, fill, learn, type Fragment, type Template } from "./capture.ts";

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
type Encode = (record: unknown, data: DataFunction[]) => Uint8Array;

/** Where a compiled route stands. */
export type RouteState =
  | { state: "learning" }
  | { state: "learned"; params: Record<string, string> }
  | { state: "served" }
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

/** Headers that belong to one response's transfer, not to the document. */
const TRANSFER = new Set([
  "transfer-encoding",
  "content-length",
  "date",
  "connection",
  "keep-alive",
]);

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

  canFill(params: Record<string, string>): boolean {
    const status = this.status;
    if (status.state === "served") return true;
    return (
      status.state === "learned" &&
      Object.entries(params).every(([name, value]) => status.params[name] === value)
    );
  }

  /** Learns from, or confirms with, a response Next.js served. */
  study(sample: Parameters<typeof learn>[0], response: Head): void {
    if (!this.template) {
      const learned = learn(sample);
      if (!learned.ok) {
        this.refuse(learned.reason);
        return;
      }
      this.template = learned.template;
      this.head = { status: response.status, headers: response.headers };
      this.status = { state: "learned", params: sample.params };
      return;
    }
    if (JSON.stringify(response.headers) !== JSON.stringify(this.head?.headers)) {
      this.refuse("Next.js's headers differ between requests");
      return;
    }
    const result = confirm(this.template, sample);
    if (!result.ok) {
      this.refuse(result.reason);
      return;
    }
    this.template = result.template;
    if (confirmed(result.template)) this.status = { state: "served" };
  }

  refuse(reason: string): void {
    this.status = { state: "refused", reason };
    this.template = undefined;
  }
}

/** A compiled route: what it needs to run, and what each variant of its requests has learned. */
export class Route {
  readonly route: CompiledRoute;
  /** Set when tsx2rs can't render the route at all: every variant is Next.js's. */
  private refused: string | undefined;
  private readonly variants = new Map<string, Variant>();
  private readonly render: Renderer;
  /** Next.js's `compression` middleware, as `next start` applies it (unless `compress: false`). */
  private readonly compress: Compress | undefined;

  constructor(route: CompiledRoute, render: Renderer, compress?: Compress) {
    this.route = route;
    this.render = render;
    this.compress = compress;
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
    const fragments = await this.render(request).catch((error: unknown) => {
      this.refused = `tsx2rs can't render it: ${error instanceof Error ? error.message : String(error)}`;
      return undefined;
    });
    if (fragments === undefined) {
      await next(req, res);
      return;
    }
    const values = {
      params: request.params,
      pathname: request.pathname,
      search: request.search,
      fragments: Object.fromEntries(fragments.map((f) => [f.name, f])),
    };
    if (variant.template && variant.head && variant.canFill(request.params)) {
      const body = fill(variant.template, values);
      if (this.compress) {
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
    // The copy is the document only uncompressed: this one response isn't compressed.
    delete req.headers["accept-encoding"];
    const learning = variant;
    await tee(req, res, next, (response) => {
      const type = response.headers["content-type"];
      const html = typeof type === "string" && type.startsWith("text/html");
      if (response.status === 200 && html)
        learning.study({ document: response.body, ...values, fragments }, response);
    });
  }
}

/** What a route renders per request: its fragments (ADR-0018). */
export type Renderer = (request: Request) => Promise<Fragment[]>;

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
  return async (request) => {
    const { program, encode, entry, callees } = await load();
    const props = {
      params: Promise.resolve(request.params),
      searchParams: Promise.resolve(request.query),
    };
    const record = await entry(props);
    const input = encode(record, callees);
    const out = program.render(Buffer.from(input.buffer, input.byteOffset, input.byteLength));
    if (out.head !== "" || out.hints !== "") {
      throw new Error("the page hoists into <head> (not yet served by tsx2rs)");
    }
    return [{ name: "page", html: out.html, flight: out.flight }];
  };
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
      done({ status: res.statusCode, headers, body: Buffer.concat(chunks).toString("utf8") });
    } finally {
      end(chunk, ...rest);
    }
    return res;
  }) as typeof res.end;
  await next(req, res);
}

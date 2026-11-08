/**
 * The tsx2rs server (milestone 1, step 8, ADR-0019): serves the routes tsx2rs compiled, and
 * hands every other request to Next.js's own request handler, the one `next start` runs (its
 * documented custom-server API). So whatever tsx2rs doesn't serve is exactly `next start`: public
 * files, static assets, prerenders and ISR, route handlers, `rsc` requests and their `_rsc` check,
 * image optimization, middleware, 404s.
 *
 * Which requests are tsx2rs's is decided with Next.js's own resolver, `@next/routing`, and the
 * build's routing rules (from the manifest `next build` wrote with `@tsx2rs/adapter`). A GET or
 * HEAD page load of a compiled route goes to its `Route` (`compiled.ts`), which learns Next.js's
 * wrapper from Next.js's own responses before it serves anything itself (ADR-0018).
 */
import { createServer, type IncomingMessage, type Server, type ServerResponse } from "node:http";
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { join } from "node:path";
import type { ResolveRoutesParams, ResolveRoutesResult } from "@next/routing";
import { compression, renderer, Route, type CompiledRoute, type RouteState } from "./compiled.ts";

// `@next/routing` is CommonJS without named ES exports.
const { resolveRoutes } = createRequire(import.meta.url)("@next/routing") as {
  resolveRoutes: (params: ResolveRoutesParams) => Promise<ResolveRoutesResult>;
};

/** `next build`'s manifest (written by `@tsx2rs/adapter`), the parts the server reads. */
export interface Manifest {
  version: number;
  nextVersion: string;
  buildId: string;
  next: {
    basePath: string;
    i18n: ResolveRoutesParams["i18n"] | null;
    routing: ResolveRoutesParams["routes"];
    outputs: { type: string; id: string; pathname: string; file: string | null; parent?: string }[];
    compress: boolean;
    /** `config.htmlLimitedBots`: the app's own pattern for bots that get blocking metadata. */
    htmlLimitedBots: string | null;
  };
  routes: (Partial<CompiledRoute> & { page: string; pathnames: string[]; compiled: boolean })[];
}

/** The manifest format this server reads. */
export const MANIFEST_VERSION = 2;
/** The Next.js version this runtime is built and tested against (ADR-0005, section 10). */
export const NEXT_VERSION = "16.4.5";

type RequestHandler = (req: IncomingMessage, res: ServerResponse) => Promise<void>;

/** Next.js's prefixes for route params and interception markers in route matches and queries. */
const PARAM = "nxtP";
const INTERCEPTION = "nxtI";

/** Next.js's custom-server API (`next`). */
type CreateNext = (options: { dir: string; dev: false; quiet?: boolean }) => {
  prepare(): Promise<void>;
  getRequestHandler(): RequestHandler;
};

export interface Options {
  /** The Next.js project's folder. */
  dir: string;
  /** Next.js's `distDir`, relative to `dir`. */
  distDir?: string;
  /** Answer `GET /__tsx2rs` with every compiled route's state (for tests; off by default). */
  status?: boolean;
}

/** Whether tsx2rs serves a request (`route`: its page's pathname), or Next.js does, and why. */
export type Decision =
  | { route: string; params: Record<string, string>; query: Record<string, string | string[]> }
  | { fallback: string };

/** Loads the build in `dir` and returns a request handler for it, and how it decides requests. */
export async function createHandler(options: Options): Promise<{
  handle: RequestHandler;
  decide: (req: IncomingMessage) => Promise<Decision>;
}> {
  const { dir } = options;
  const distDir = options.distDir ?? ".next";
  const manifest = JSON.parse(
    readFileSync(join(dir, distDir, "tsx2rs", "manifest.json"), "utf8"),
  ) as Manifest;
  if (manifest.version !== MANIFEST_VERSION) {
    throw new Error(
      `manifest version ${String(manifest.version)}; this runtime reads ${String(MANIFEST_VERSION)}`,
    );
  }
  const app = createRequire(join(dir, "package.json"));
  const nextVersion = (app("next/package.json") as { version: string }).version;
  const { next: build } = manifest;
  const pathnames = build.outputs.map((o) => o.pathname);
  // Nothing is served by tsx2rs for an app built with another Next.js, or one with middleware
  // (it would have to run before compiled routes; not yet).
  const disabled =
    nextVersion !== NEXT_VERSION || manifest.nextVersion !== NEXT_VERSION
      ? `Next.js ${nextVersion} (tsx2rs supports ${NEXT_VERSION})`
      : build.outputs.some((o) => o.type === "MIDDLEWARE")
        ? "the app has middleware (proxy.ts)"
        : undefined;

  const manifestDir = join(dir, distDir, "tsx2rs");
  const compiled = new Map<string, Route>();
  for (const r of manifest.routes) {
    if (!r.compiled || !r.entrypoint || !r.program || !r.encoder || !r.entry || !r.callees)
      continue;
    const route = new Route(
      r as CompiledRoute,
      renderer(r as CompiledRoute, dir, manifestDir),
      build.compress ? compression(dir) : undefined,
    );
    for (const pathname of r.pathnames)
      if (!pathname.endsWith(".rsc")) compiled.set(pathname, route);
  }

  const { getBotType } = app("next/dist/shared/lib/router/utils/is-bot.js") as {
    getBotType: BotType;
  };
  const htmlLimitedBots = build.htmlLimitedBots
    ? new RegExp(build.htmlLimitedBots, "i")
    : undefined;

  const createNext = app("next") as CreateNext;
  const nextApp = createNext({ dir, dev: false, quiet: true });
  await nextApp.prepare();
  const next = nextApp.getRequestHandler();

  async function decide(req: IncomingMessage): Promise<Decision> {
    if (disabled) return { fallback: disabled };
    if (req.method !== "GET" && req.method !== "HEAD")
      return { fallback: `a ${req.method ?? ""} request` };
    if (req.headers["rsc"] !== undefined) return { fallback: "a client navigation (rsc)" };
    const headers = new Headers();
    for (const [name, value] of Object.entries(req.headers)) {
      if (value !== undefined) headers.set(name, Array.isArray(value) ? value.join(", ") : value);
    }
    const resolved = await resolveRoutes({
      url: new URL(req.url ?? "/", `http://${req.headers.host ?? "localhost"}`),
      buildId: manifest.buildId,
      basePath: build.basePath,
      requestBody: new ReadableStream(),
      headers,
      pathnames,
      ...(build.i18n ? { i18n: build.i18n } : {}),
      routes: build.routing,
      invokeMiddleware: () => Promise.resolve({}),
    });
    return decision(resolved, compiled);
  }

  async function handle(req: IncomingMessage, res: ServerResponse): Promise<void> {
    if (options.status && req.url === "/__tsx2rs") {
      const states: Record<string, RouteState & { variants?: Record<string, RouteState> }> = {};
      for (const [pathname, route] of compiled)
        states[pathname] = { ...route.state, variants: route.states };
      res.writeHead(200, { "content-type": "application/json" }).end(JSON.stringify(states));
      return;
    }
    const decision = await decide(req);
    if ("fallback" in decision) {
      await next(req, res);
      return;
    }
    const url = new URL(req.url ?? "/", "http://localhost");
    await compiled.get(decision.route)?.handle(
      req,
      res,
      {
        variant: variant(req.headers, getBotType, htmlLimitedBots),
        params: decision.params,
        pathname: url.pathname,
        search: url.search,
        query: decision.query,
      },
      next,
    );
  }

  return { handle, decide };
}

/**
 * What `resolveRoutes` decided for a page load: the compiled route tsx2rs serves, with the params
 * and query Next.js gives its page, or why Next.js serves it.
 */
export function decision(
  resolved: ResolveRoutesResult,
  compiled: ReadonlyMap<string, unknown>,
): Decision {
  if (resolved.resolvedPathname === undefined) return { fallback: "no route" };
  if (!compiled.has(resolved.resolvedPathname)) {
    return { fallback: `${resolved.resolvedPathname} isn't compiled` };
  }
  if (
    resolved.redirect ||
    resolved.externalRewrite ||
    resolved.resolvedHeaders?.keys().next().done === false
  ) {
    return { fallback: "routing rules apply (redirect, rewrite or headers)" };
  }
  // Next.js names dynamic params `nxtP<name>` in route matches (with the undecoded value the page
  // gets) and in the query (decoded), next to regex groups; `nxtI` marks interception routes.
  const params: Record<string, string> = {};
  for (const [key, value] of Object.entries(resolved.routeMatches ?? {})) {
    if (key.startsWith(PARAM)) params[key.slice(PARAM.length)] = value;
  }
  const query = Object.fromEntries(
    Object.entries(resolved.invocationTarget?.query ?? {}).filter(
      ([key]) => !key.startsWith(PARAM) && !key.startsWith(INTERCEPTION),
    ),
  );
  return { route: resolved.resolvedPathname, params, query };
}

/** Next.js's `getBotType`: `"dom"`, `"html"` or `undefined` for a user agent. */
export type BotType = (userAgent: string) => string | undefined;

/**
 * A request's variant (ADR-0018): what Next.js's wrapper may depend on besides the URL and
 * params. The names of the cookies sent (a layout reading a cart cookie renders differently),
 * and the kind of client by Next.js's own bot rules (bots get blocking metadata).
 */
export function variant(
  headers: IncomingMessage["headers"],
  getBotType: BotType,
  htmlLimitedBots?: RegExp,
): string {
  const cookies = (headers.cookie ?? "")
    .split(";")
    .map((c) => c.split("=")[0]?.trim() ?? "")
    .filter((name) => name !== "")
    .sort();
  const ua = headers["user-agent"] ?? "";
  const bot = (htmlLimitedBots?.test(ua) ? "html" : getBotType(ua)) ?? "";
  return cookies.length === 0 && bot === ""
    ? ""
    : JSON.stringify({ cookies: [...new Set(cookies)], bot });
}

/** Starts the server for the app in `dir`. */
export async function start(options: Options & { port: number; host?: string }): Promise<Server> {
  const { handle } = await createHandler(options);
  const server = createServer((req, res) => {
    handle(req, res).catch((error: unknown) => {
      console.error(error);
      if (!res.headersSent) res.writeHead(500).end();
      else res.destroy();
    });
  });
  return new Promise((resolve) => {
    server.listen(options.port, options.host ?? "127.0.0.1", () => {
      resolve(server);
    });
  });
}

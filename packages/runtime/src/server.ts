/**
 * The tsx2rs server (milestone 1, step 8, ADR-0019): serves the routes tsx2rs compiled, and
 * hands every other request to Next.js's own request handler, the one `next start` runs (its
 * documented custom-server API). So whatever tsx2rs doesn't serve is exactly `next start`: public
 * files, static assets, prerenders and ISR, route handlers, `rsc` requests and their `_rsc` check,
 * image optimization, middleware, 404s.
 *
 * Which requests are tsx2rs's is decided with Next.js's own resolver, `@next/routing`, and the
 * build's routing rules (from the manifest `next build` wrote with `@tsx2rs/adapter`). Serving
 * compiled routes comes in step 8.3; until then every request goes to Next.js.
 */
import { createServer, type IncomingMessage, type Server, type ServerResponse } from "node:http";
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { join } from "node:path";
import type { ResolveRoutesParams, ResolveRoutesResult } from "@next/routing";

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
  };
}

/** The manifest format this server reads. */
export const MANIFEST_VERSION = 2;
/** The Next.js version this runtime is built and tested against (ADR-0005, section 10). */
export const NEXT_VERSION = "16.4.3";

type RequestHandler = (req: IncomingMessage, res: ServerResponse) => Promise<void>;

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
}

/** Whether tsx2rs serves a request (`route`: its page's pathname), or Next.js does, and why. */
export type Decision = { route: string } | { fallback: string };

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
    if (resolved.resolvedPathname === undefined) return { fallback: "no route" };
    // Serving compiled routes comes with step 8.3.
    return { fallback: `${resolved.resolvedPathname} isn't served by tsx2rs yet` };
  }

  async function handle(req: IncomingMessage, res: ServerResponse): Promise<void> {
    const decision = await decide(req);
    if ("fallback" in decision) await next(req, res);
  }

  return { handle, decide };
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

/**
 * Writes a compiled route's program and one request's engine input, for the engine's benchmarks
 * (`crates/engine/benches/cases/commerce/`): the real page, not a synthetic one.
 *
 *     node packages/runtime/scripts/bench-case.ts <app> <page> <params as JSON> <out folder>
 *     node packages/runtime/scripts/bench-case.ts tests/apps/commerce \
 *       'app/product/[handle]/page.tsx' '{"handle":"acme-hoodie"}' crates/engine/benches/cases/commerce
 *
 * The app must be built with `@tsx2rs/adapter`. Islands and Suspense boundaries get the runtime's
 * placeholders (ADR-0020, ADR-0024): the engine's work is the same, and the input doesn't depend
 * on what a server learned. Run it again after a change to the program or input format.
 */
import { AsyncLocalStorage } from "node:async_hooks";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { join, resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { placeholders } from "../src/islands.ts";

const [appArg, page, paramsJson, outArg] = process.argv.slice(2);
if (!appArg || !page || !paramsJson || !outArg) {
  console.error(
    "usage: node packages/runtime/scripts/bench-case.ts <app> <page> <params as JSON> <out folder>",
  );
  process.exit(2);
}
const app = resolve(appArg);
const out = resolve(outArg);

// Next.js's bundle expects what its server sets up: AsyncLocalStorage and the app's `.env`.
(globalThis as Record<string, unknown>)["AsyncLocalStorage"] = AsyncLocalStorage;
const require = createRequire(join(app, "package.json"));
(
  createRequire(require.resolve("next"))("@next/env") as { loadEnvConfig: (dir: string) => void }
).loadEnvConfig(app);

interface Name {
  module: string;
  name: string;
}
interface Route {
  page: string;
  entrypoint: string;
  program: string;
  encoder: string;
  entry: Name;
  callees: Name[];
}
const manifest = JSON.parse(readFileSync(join(app, ".next/tsx2rs/manifest.json"), "utf8")) as {
  routes: Route[];
};
const route = manifest.routes.find((r) => r.page === page);
if (!route) throw new Error(`no compiled route ${page}`);

// Evaluating the route's modules registers their data functions (ADR-0017).
const mod = require(join(app, route.entrypoint)) as {
  routeModule: { userland: { loaderTree: unknown } };
};
async function evaluate(node: unknown): Promise<void> {
  if (Array.isArray(node) && typeof node[0] === "function" && typeof node[1] === "string") {
    await (node[0] as () => Promise<unknown>)();
    return;
  }
  if (node !== null && typeof node === "object") {
    for (const value of Object.values(node)) await evaluate(value);
  }
}
await evaluate(mod.routeModule.userland.loaderTree);
type DataFunction = (props: unknown) => unknown;
const registry = (globalThis as Record<symbol, Map<string, Record<string, DataFunction>>>)[
  Symbol.for("tsx2rs.data")
];
const fn = (f: Name): DataFunction => {
  const found = registry?.get(f.module)?.[f.name];
  if (!found) throw new Error(`${f.module}: ${f.name} isn't in the server bundle`);
  return found;
};

const encoder = (await import(pathToFileURL(join(app, ".next/tsx2rs", route.encoder)).href)) as {
  encode: (...args: unknown[]) => Uint8Array | Promise<Uint8Array>;
};
const params = JSON.parse(paramsJson) as Record<string, string>;
const record = await fn(route.entry)({
  params: Promise.resolve(params),
  searchParams: Promise.resolve({}),
});
const parts = placeholders();
const input = await encoder.encode(record, route.callees.map(fn), parts.islands, parts.boundaries);

mkdirSync(out, { recursive: true });
writeFileSync(join(out, "program.bin"), readFileSync(join(app, ".next/tsx2rs", route.program)));
writeFileSync(join(out, "input.bin"), input);
console.log(
  `${route.page} ${paramsJson}: program and ${String(input.byteLength)} input bytes in ${out}`,
);
process.exit(0);

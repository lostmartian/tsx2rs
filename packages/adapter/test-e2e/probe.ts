/**
 * A runtime's view of a build (ADR-0017), run as its own process with the app as its working
 * directory: load a compiled route's entrypoint as Next.js's server does, evaluate its modules
 * through the loader tree, find the data functions the loader registered, run the page's, and
 * encode its record with the route's generated encoder. Prints the result as JSON.
 */
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { join } from "node:path";
import { pathToFileURL } from "node:url";

interface Route {
  page: string;
  entrypoint: string;
  encoder: string;
  entry: { module: string; name: string };
  callees: { module: string; name: string }[];
}

const app = process.cwd();
const require = createRequire(join(app, "package.json"));
require("next/dist/build/adapter/setup-node-env.external.js");
const manifest = JSON.parse(readFileSync(join(app, ".next/tsx2rs/manifest.json"), "utf8")) as {
  routes: (Route & { compiled: boolean })[];
};
const routes = manifest.routes.filter((r) => r.compiled);

type Getter = () => Promise<unknown>;
async function load(node: unknown): Promise<void> {
  if (Array.isArray(node) && typeof node[0] === "function" && typeof node[1] === "string") {
    await (node[0] as Getter)();
    return;
  }
  if (node !== null && typeof node === "object") for (const v of Object.values(node)) await load(v);
}
type Data = (props: unknown) => unknown;
const registry = (): Map<string, Record<string, Data>> | undefined =>
  (globalThis as Record<symbol, Map<string, Record<string, Data>> | undefined>)[
    Symbol.for("tsx2rs.data")
  ];
const fn = (f: { module: string; name: string }): Data => {
  const found = registry()?.get(f.module)?.[f.name];
  if (!found) throw new Error(`${f.module}: ${f.name} isn't registered`);
  return found;
};

const results = [];
for (const route of routes) {
  const entry = require(join(app, route.entrypoint)) as {
    routeModule: { userland: { loaderTree: unknown } };
  };
  await load(entry.routeModule.userland.loaderTree);
  const record = (await fn(route.entry)({ params: Promise.resolve({ n: "3" }) })) as unknown[];
  const encoder = (await import(pathToFileURL(join(app, ".next/tsx2rs", route.encoder)).href)) as {
    encode(record: unknown[], data: Data[]): Uint8Array;
  };
  try {
    const input = encoder.encode(record, route.callees.map(fn));
    results.push({ page: route.page, input: Buffer.from(input).toString("hex") });
  } catch (error) {
    // A value the engine doesn't take natively (a client component): handed to React (step 8).
    results.push({ page: route.page, notNative: (error as Error).message });
  }
}
console.log(JSON.stringify({ registered: [...(registry()?.keys() ?? [])].sort(), results }));

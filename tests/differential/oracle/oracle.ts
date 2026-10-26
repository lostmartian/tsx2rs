/**
 * The React oracle for the engine's differential tests. It reads one JSON request per line on
 * stdin and writes one JSON response per line on stdout. Two modes, because React's Flight
 * server only loads under the `react-server` condition and React DOM's server only without it:
 *
 *   node --conditions=react-server oracle.ts flight
 *     request: {"tree": Tree}                     response: {"payload": string}
 *   node oracle.ts html
 *     request: {"tree", "engineHtml", "engineFlight", "reactPayload"}
 *     response: {"ok": true} or {"ok": false, "html"?: difference, "flight"?: difference}
 *     request: {"tree"}  (the engine returned an error)
 *     response: {"reactHtml": string} or {"reactThrew": message}
 *
 * In html mode the oracle is also the judge: React DOM's HTML must equal the engine's byte for
 * byte, and React Flight's payload must decode to the same tree as the engine's Flight value
 * (decoded with the identity harness, ADR-0002). When React DOM throws, the response says so
 * (`reactThrew`): the engine must then return an error too.
 */
import { createRequire } from "node:module";
import { createInterface } from "node:readline";
import { Writable } from "node:stream";

/** A generated React tree (see tests/differential/src/tree.rs for the generator). */
export type Tree =
  | string
  | null
  | boolean
  | { n: string }
  | { u: 1 }
  | { b: string }
  | Tree[]
  /** A function component that returns this tree (a compiled component, ADR-0008). */
  | { f: Tree }
  | {
      t: string;
      /** The key, if any. */
      k?: string;
      a: [string, AttributeValue][];
      c: Tree[];
      /** Where `children` goes among the props; absent: after them, as JSX writes it. */
      ci?: number;
    };
type AttributeValue =
  | string
  | boolean
  | null
  | { u: 1 }
  | { n: string }
  | { b: string }
  | { s: [string, AttributeValue][] }
  | { a: AttributeValue[] }
  | { h: AttributeValue };

const require = createRequire(import.meta.url);

interface ReactModule {
  createElement(
    type: string | (() => unknown),
    props: Record<string, unknown> | null,
    ...children: unknown[]
  ): unknown;
}

function toValue(value: AttributeValue): unknown {
  if (value === null || typeof value !== "object") return value;
  if ("n" in value) return Number(value.n);
  if ("b" in value) return BigInt(value.b);
  if ("s" in value) return Object.fromEntries(value.s.map(([k, v]) => [k, toValue(v)]));
  if ("a" in value) return value.a.map(toValue);
  if ("h" in value) return { __html: toValue(value.h) };
  return undefined;
}

function toElement(react: ReactModule, tree: Tree): unknown {
  if (tree === null || typeof tree === "string" || typeof tree === "boolean") return tree;
  if (Array.isArray(tree)) return tree.map((node) => toElement(react, node));
  if ("n" in tree) return Number(tree.n); // "NaN", "-0", "Infinity" and decimals all parse
  if ("u" in tree) return undefined;
  if ("b" in tree) return BigInt(tree.b);
  if ("f" in tree) {
    const output = toElement(react, tree.f);
    return react.createElement(() => output, null);
  }
  const props: Record<string, unknown> = {};
  if (tree.k !== undefined) props["key"] = tree.k;
  const children = tree.c.map((c) => toElement(react, c));
  if (tree.ci === undefined) {
    for (const [name, value] of tree.a) props[name] = toValue(value);
    return react.createElement(tree.t, props, ...children);
  }
  // `children` as a prop, at its place in the props object.
  tree.a.forEach(([name, value], i) => {
    if (i === tree.ci) props["children"] = children.length === 1 ? children[0] : children;
    props[name] = toValue(value);
  });
  if (tree.ci === tree.a.length) props["children"] = children.length === 1 ? children[0] : children;
  return react.createElement(tree.t, props);
}

async function flightPayload(tree: Tree): Promise<string> {
  const react = require("react") as ReactModule;
  const server = require("react-server-dom-webpack/server") as {
    renderToPipeableStream(model: unknown, webpackMap: null): { pipe(w: Writable): void };
  };
  const chunks: Buffer[] = [];
  await new Promise<void>((resolve, reject) => {
    const sink = new Writable({
      write(chunk: Buffer, _encoding, done) {
        chunks.push(chunk);
        done();
      },
      final(done) {
        resolve();
        done();
      },
    });
    sink.on("error", reject);
    server.renderToPipeableStream(toElement(react, tree), null).pipe(sink);
  });
  return Buffer.concat(chunks).toString("utf8");
}

interface JudgeRequest {
  tree: Tree;
  engineHtml?: string;
  engineFlight?: string;
  reactPayload?: string;
}

async function judge(request: JudgeRequest): Promise<Record<string, unknown>> {
  const react = require("react") as ReactModule;
  const { renderToString } = require("react-dom/server") as {
    renderToString: (element: unknown) => string;
  };
  let reactHtml: string;
  try {
    reactHtml = renderToString(toElement(react, request.tree));
  } catch (e) {
    const reactThrew = e instanceof Error ? e.message : String(e);
    return request.engineHtml === undefined ? { reactThrew } : { ok: false, reactThrew };
  }
  if (request.engineHtml === undefined || request.engineFlight === undefined) {
    return { reactHtml };
  }
  const identity = await import("@tsx2rs/identity");
  const html = identity.firstHtmlDifference(reactHtml, request.engineHtml);
  const encoder = new TextEncoder();
  const reactTree = identity.decodeFlight(encoder.encode(request.reactPayload ?? "")).root;
  const engineTree = identity.decodeFlight(encoder.encode(`0:${request.engineFlight}\n`)).root;
  const flight = identity.firstTreeDifference(reactTree, engineTree, []);
  return html === undefined && flight === undefined
    ? { ok: true }
    : {
        ok: false,
        ...(html && { html }),
        ...(flight && { flight }),
        reactHtml,
        reactPayload: request.reactPayload,
      };
}

const mode = process.argv[2];
if (mode !== "flight" && mode !== "html") {
  console.error("usage: oracle.ts flight|html");
  process.exit(2);
}
for await (const line of createInterface({ input: process.stdin })) {
  let response: Record<string, unknown>;
  try {
    const request = JSON.parse(line) as JudgeRequest;
    response =
      mode === "flight" ? { payload: await flightPayload(request.tree) } : await judge(request);
  } catch (e) {
    response = { ok: false, error: e instanceof Error ? (e.stack ?? e.message) : String(e) };
  }
  process.stdout.write(JSON.stringify(response) + "\n");
}

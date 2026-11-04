/**
 * The Node.js half of the component fixtures (ADR-0012, section 6): a compiled component's data
 * functions are run here, their records resolved and encoded as the engine's input, and React
 * renders the original component for comparison. One JSON request per line on stdin, one JSON
 * response per line on stdout:
 *
 *   node oracle/components.ts html
 *     {"op": "cases", "source"}                 → {"cases": number}
 *     {"op": "input", "data", "encoder", "callees", "plan", "case"}
 *                                               → {"input": hex} or {"error": string}
 *     {"op": "judge", "source", "case", "engineHtml", "engineFlight", "engineHints",
 *      "reactPayload"}                          → {"ok": true} or {"ok": false, …differences}
 *   node --conditions=react-server oracle/components.ts flight
 *     {"op": "flight", "source", "case"}        → {"payload": string} or {"error": string}
 *
 * `source` is a fixture (`.tsx`): its default export is the page, and `export const cases` its
 * props, one object per case. `data` is the module the compiler generated: the fixture's source
 * with its data functions appended. Both are compiled with the SWC Next.js uses.
 *
 * The judge renders the page with React DOM and compares: the HTML byte for byte, and React's
 * Flight payload with the engine's hint rows then its model row (`0:…`), as one string.
 *
 * `encoder` is the route's generated encoder (ADR-0016), run on `@tsx2rs/runtime/encode` with
 * the data functions named in `callees`. The record is also resolved and encoded here, by a
 * reference written from the record's contract (ADR-0012 to ADR-0015) and React's rules for key
 * paths (ADR-0008) that interprets `plan`: the two must write the same bytes.
 */
import { createRequire } from "node:module";
import { createInterface } from "node:readline";
import { Writable } from "node:stream";

const require = createRequire(import.meta.url);

/** How a record's slots are read: the compiler's `lower::Slot`, as the fixture test writes it. */
type Slot =
  | { v: "prop" | "style" | "flight" | "child" }
  | { p: 1 }
  | { b: "truthy" | "notnull" | "present"; root: boolean; then: Slot[]; else: Slot[] }
  | { l: Slot }
  | { r: Slot[][] }
  | { k: boolean; body: Slot[] }
  | { rw: 1 }
  | { c: string; root: boolean; body?: Slot[]; t?: number; slots: Record<string, Slot[]> }
  | { pl: Slot[] };

interface Plan {
  /** The entry's data function. */
  entry: string;
  record: Slot[];
  /** The records of templates 1.. (`Op::Call`). */
  templates: Slot[][];
}

type Module = Record<string, unknown>;
type DataFunction = (props: unknown) => unknown[];

interface Swc {
  loadBindings?: () => Promise<void>;
  transform(source: string, options: object): Promise<{ code: string }>;
}
const swc = require("next/dist/build/swc/index.js") as Swc;
const modules = new Map<string, Module>();

/** Compiles a module with Next.js's SWC (TSX, React's automatic runtime) and runs it. */
async function load(source: string): Promise<Module> {
  const cached = modules.get(source);
  if (cached) return cached;
  await swc.loadBindings?.();
  const { code } = await swc.transform(source, {
    filename: "page.tsx",
    jsc: {
      parser: { syntax: "typescript", tsx: true },
      transform: { react: { runtime: "automatic", development: false } },
      target: "es2022",
    },
    module: { type: "commonjs" },
  });
  const module = { exports: {} as Module };
  // eslint-disable-next-line @typescript-eslint/no-implied-eval -- the fixture's own code
  const run = new Function("require", "module", "exports", code) as (
    require: NodeJS.Require,
    module: { exports: Module },
    exports: Module,
  ) => void;
  run(require, module, module.exports);
  modules.set(source, module.exports);
  return module.exports;
}

async function cases(source: string): Promise<unknown[]> {
  const loaded = await load(source);
  if (!Array.isArray(loaded["cases"])) throw new Error("the fixture has no `export const cases`");
  return loaded["cases"] as unknown[];
}

/** The engine's input format (`tsx2rs_ir::input`). */
class Input {
  private bytes: number[] = [];
  u8(n: number): void {
    this.bytes.push(n);
  }
  u32(n: number): void {
    this.bytes.push(n & 0xff, (n >>> 8) & 0xff, (n >>> 16) & 0xff, (n >>> 24) & 0xff);
  }
  f64(n: number): void {
    const b = Buffer.alloc(8);
    b.writeDoubleLE(n);
    this.bytes.push(...b);
  }
  string(s: string): void {
    const b = Buffer.from(s, "utf8");
    this.u32(b.length);
    this.bytes.push(...b);
  }
  hex(): string {
    return Buffer.from(this.bytes).toString("hex");
  }
}

const TAG = {
  undefined: 0,
  null: 1,
  false: 2,
  true: 3,
  number: 4,
  string: 5,
  array: 6,
  object: 7,
  bigint: 8,
  element: 9,
};

class Unsupported extends Error {}

function primitive(w: Input, v: unknown): void {
  if (v === undefined) w.u8(TAG.undefined);
  else if (v === null) w.u8(TAG.null);
  else if (v === false) w.u8(TAG.false);
  else if (v === true) w.u8(TAG.true);
  else if (typeof v === "number") {
    w.u8(TAG.number);
    w.f64(v);
  } else if (typeof v === "string") {
    w.u8(TAG.string);
    w.string(v);
  } else if (typeof v === "bigint") {
    w.u8(TAG.bigint);
    w.string(String(v));
  } else throw new Unsupported(`not a primitive: ${typeof v}`);
}

function style(w: Input, v: unknown): void {
  if (v === undefined || v === null) {
    primitive(w, v);
    return;
  }
  if (typeof v !== "object") throw new Unsupported("a style that isn't an object");
  const entries = Object.entries(v);
  w.u8(TAG.object);
  w.u32(entries.length);
  for (const [name, value] of entries) {
    w.string(name);
    primitive(w, value);
  }
}

/** `Op::Value`: any value React's Flight server writes natively. */
function value(w: Input, v: unknown): void {
  if (Array.isArray(v)) {
    w.u8(TAG.array);
    w.u32(v.length);
    for (const item of v) value(w, item);
  } else if (v !== null && typeof v === "object") {
    const entries = Object.entries(v);
    w.u8(TAG.object);
    w.u32(entries.length);
    for (const [k, item] of entries) {
      w.string(k);
      value(w, item);
    }
  } else primitive(w, v);
}

interface ReactElement {
  $$typeof: symbol;
  type: unknown;
  key: string | null;
  props: Record<string, unknown>;
}
const isElement = (v: unknown): v is ReactElement =>
  v !== null && typeof v === "object" && "$$typeof" in v && typeof v.$$typeof === "symbol";

/** `Op::Child`: a React node. Host elements become element nodes (ADR-0007). */
function node(w: Input, v: unknown): void {
  if (Array.isArray(v)) {
    w.u8(TAG.array);
    w.u32(v.length);
    for (const item of v) node(w, item);
  } else if (isElement(v)) {
    if (typeof v.type !== "string") throw new Unsupported("a component element as a value");
    w.u8(TAG.element);
    w.string(v.type);
    if (v.key === null) w.u8(TAG.null);
    else {
      w.u8(TAG.string);
      w.string(v.key);
    }
    const entries = Object.entries(v.props);
    w.u32(entries.length);
    for (const [name, prop] of entries) {
      w.string(name);
      if (name === "children") continue; // a marker: the node follows the last entry
      value(w, prop); // tagged: a primitive, a style object, `{ __html }`, or an array
    }
    if ("children" in v.props) node(w, v.props["children"]);
  } else primitive(w, v);
}

/** React's key path state for a component's output (ADR-0008). */
interface Keys {
  path: string | null;
  implicit: boolean;
  /** The root element's wrap, decided at its first `RootWrap`. */
  wrap?: boolean;
}

/** React 19's `jsx(type, config, key)`: the key and the props. */
function jsxArguments(config: Record<string, unknown>, key: unknown): [string | null, object] {
  // `"" + key`, as React writes it.
  const text = (k: unknown): string => String(k);
  let resolved = key === undefined ? null : text(key);
  if (config["key"] !== undefined) resolved = text(config["key"]);
  const props = Object.fromEntries(Object.entries(config).filter(([k]) => k !== "key"));
  return [resolved, props];
}

/**
 * Walks a record in program order and writes the input the program reads, resolving pending
 * calls and key paths as the runtime will. A data function returns its whole record before any
 * call in it is resolved, so resolving calls as the walk reaches them is React's order: each
 * child after its parent, depth first.
 */
class Resolver {
  private readonly data: Module;
  private readonly plan: Plan;
  private readonly w: Input;

  constructor(data: Module, plan: Plan, w: Input) {
    this.data = data;
    this.plan = plan;
    this.w = w;
  }

  record(slots: Slot[], values: unknown[], keys: Keys): void {
    let i = 0;
    for (const slot of slots) {
      if ("rw" in slot) {
        // Read before the root element (whose own key is the next value) and after it; React
        // wraps a keyed element in an implicit slot (ADR-0008).
        keys.wrap ??= keys.implicit && (values[i] as unknown[])[0] !== null;
        this.w.u8(keys.wrap ? 1 : 0);
        continue;
      }
      this.slot(slot, values[i++], keys);
    }
  }

  private slot(slot: Slot, v: unknown, keys: Keys): void {
    const w = this.w;
    if ("v" in slot) {
      if (slot.v === "child") node(w, v);
      else if (slot.v === "prop") primitive(w, v);
      else if (slot.v === "style") style(w, v);
      else value(w, v);
    } else if ("p" in slot) {
      const entries = Object.entries(v as object).filter(([k]) => k !== "key" && k !== "children");
      w.u32(entries.length);
      for (const [name, prop] of entries) {
        w.string(name);
        if (name === "style") style(w, prop);
        else primitive(w, prop);
      }
    } else if ("k" in slot) {
      // `[key, ...record]`: a root element's key gets the key path in front (ADR-0008).
      const [own, ...record] = v as [string | null, ...unknown[]];
      const key =
        !slot.k || keys.path === null ? own : own === null ? keys.path : `${keys.path},${own}`;
      primitive(w, key);
      this.record(slot.body, record, { path: null, implicit: false });
    } else if ("b" in slot && slot.b === "present") {
      // A list item that reads nothing else: an always-true condition (no value of its own).
      w.u8(1);
      this.record(slot.then, [v], { path: null, implicit: false });
    } else if ("b" in slot) {
      const [taken, record] = v as [boolean, unknown[]];
      w.u8(taken ? 1 : 0);
      // At a component's root, what a branch renders is the component's own output.
      const inner = slot.root ? keys : { path: null, implicit: false };
      this.record(taken ? slot.then : slot.else, record, inner);
    } else if ("l" in slot) {
      const items = v as unknown[];
      w.u32(items.length);
      for (const item of items) this.record([slot.l], [item], { path: null, implicit: false });
    } else if ("r" in slot) {
      // One return is just its record; several are `[index, record]`, read as a chain of `If`s.
      if (slot.r.length === 1) {
        this.record(slot.r[0] ?? [], v as unknown[], keys);
        return;
      }
      const [index, record] = v as [number, unknown[]];
      for (let j = 0; j < slot.r.length - 1 && j <= index; j++) w.u8(j === index ? 1 : 0);
      this.record(slot.r[index] ?? [], record, keys);
    } else if ("c" in slot) {
      const [config, key] = v as [Record<string, unknown>, unknown];
      const [resolved, props] = jsxArguments(config, key);
      const outer = slot.root ? keys : { path: null, implicit: false };
      const callee: Keys =
        resolved !== null
          ? {
              path: outer.path === null ? resolved : `${outer.path},${resolved}`,
              implicit: outer.implicit,
            }
          : { path: outer.path, implicit: outer.path === null ? true : outer.implicit };
      const fn = this.data[slot.c] as DataFunction | undefined;
      if (typeof fn !== "function") throw new Error(`no data function ${slot.c}`);
      const body = slot.body ?? this.plan.templates[(slot.t ?? 1) - 1] ?? [];
      this.record(body, fn(props), callee);
    } else if ("pl" in slot) {
      // The callee places the slot's record, built by the caller.
      this.record(slot.pl, v as unknown[], { path: null, implicit: false });
    }
  }
}

async function input(dataSource: string, plan: Plan, props: unknown): Promise<string> {
  const data = await load(dataSource);
  const fn = data[plan.entry] as DataFunction | undefined;
  if (typeof fn !== "function") throw new Error(`no data function ${plan.entry}`);
  const w = new Input();
  // Next.js renders a page as an unkeyed function component.
  new Resolver(data, plan, w).record(plan.record, fn(props), { path: null, implicit: true });
  return w.hex();
}

type Encode = (record: unknown[], data: unknown[]) => Uint8Array;

/** The input the generated encoder writes for the entry's record. */
async function encode(request: Request, props: unknown): Promise<string> {
  const data = await load(request.data);
  const encoder = await load(request.encoder);
  const fn = data[request.plan.entry] as DataFunction;
  const callees = request.callees.map((name) => data[name]);
  return Buffer.from((encoder["encode"] as Encode)(fn(props), callees)).toString("hex");
}

interface React {
  createElement(type: unknown, props: unknown): unknown;
}

async function page(source: string, index: number): Promise<unknown> {
  const loaded = await load(source);
  const react = require("react") as React;
  return react.createElement(loaded["default"], (await cases(source))[index]);
}

async function flight(source: string, index: number): Promise<string> {
  const server = require("react-server-dom-webpack/server") as {
    renderToPipeableStream(model: unknown, webpackMap: null): { pipe(w: Writable): void };
  };
  const element = await page(source, index);
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
    server.renderToPipeableStream(element, null).pipe(sink);
  });
  return Buffer.concat(chunks).toString("utf8");
}

interface Request {
  op: "cases" | "input" | "judge" | "flight";
  source: string;
  data: string;
  encoder: string;
  callees: string[];
  plan: Plan;
  case: number;
  engineHtml: string;
  engineFlight: string;
  engineHints: string;
  reactPayload: string;
}

async function answer(request: Request): Promise<object> {
  try {
    switch (request.op) {
      case "cases":
        return { cases: (await cases(request.source)).length };
      case "input": {
        const props = (await cases(request.source))[request.case];
        const reference = await input(request.data, request.plan, props);
        const generated = await encode(request, props);
        if (generated !== reference) {
          return {
            error: "the generated encoder differs from the reference",
            generated,
            reference,
          };
        }
        return { input: generated };
      }
      case "judge": {
        const server = require("react-dom/server") as {
          renderToString: (element: unknown) => string;
        };
        const html = server.renderToString(await page(request.source, request.case));
        const engineFlight = `${request.engineHints}0:${request.engineFlight}\n`;
        const verdict: Record<string, unknown> = {};
        if (html !== request.engineHtml)
          verdict["html"] = { react: html, engine: request.engineHtml };
        if (request.reactPayload !== engineFlight) {
          verdict["flight"] = { react: request.reactPayload, engine: engineFlight };
        }
        return Object.keys(verdict).length === 0 ? { ok: true } : { ok: false, ...verdict };
      }
      case "flight":
        return { payload: await flight(request.source, request.case) };
    }
  } catch (error) {
    return { error: error instanceof Error ? error.message : String(error) };
  }
}

const lines = createInterface({ input: process.stdin });
for await (const line of lines) {
  process.stdout.write(`${JSON.stringify(await answer(JSON.parse(line) as Request))}\n`);
}

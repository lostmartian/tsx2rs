/**
 * Encoding records as the engine's input (ADR-0016). A compiled route's generated encoder walks
 * its data functions' records in program order and writes each value with the encoder for its
 * hole kind (ADR-0005, section 5), in the format `tsx2rs_ir::input` defines. Resolving pending
 * calls (ADR-0013) and key paths (ADR-0008) happens on the same walk.
 *
 * A value the engine can't take natively throws {@link NotNative}: the route hands it to React
 * (ADR-0005, section 10).
 */

/** The input format's tags (`tsx2rs_ir::input::tag`). */
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
  reference: 11,
  identity: 12,
} as const;

/** A value the engine can't take natively, and why. */
export class NotNative extends Error {
  readonly value: unknown;

  constructor(reason: string, value: unknown) {
    super(reason);
    this.name = "NotNative";
    this.value = value;
  }
}

/**
 * Where the next input's buffer starts: the size the last one needed, so a page's input is
 * written without growing (between 4 KiB and 1 MiB).
 */
let nextSize = 4096;

/** The engine's input, written in program order. */
export class Input {
  // A Node `Buffer` of its own (not from the shared pool), written without zeroing first: only
  // what's written is read.
  private buffer = Buffer.allocUnsafeSlow(nextSize);
  private view = new DataView(this.buffer.buffer);
  private length = 0;

  private reserve(n: number): void {
    if (this.length + n <= this.buffer.length) return;
    let size = this.buffer.length * 2;
    while (size < this.length + n) size *= 2;
    const next = Buffer.allocUnsafeSlow(size);
    this.buffer.copy(next, 0, 0, this.length);
    this.buffer = next;
    this.view = new DataView(next.buffer);
  }

  u8(n: number): void {
    this.reserve(1);
    this.buffer[this.length++] = n;
  }

  /** The objects written with an identity, and theirs. */
  private readonly identities = new Map<object, number>();

  /** An object's identity (`tag::IDENTITY`): the same for the same object in one input. */
  identity(object: object): void {
    let id = this.identities.get(object);
    if (id === undefined) {
      id = this.identities.size;
      this.identities.set(object, id);
    }
    this.u8(TAG.identity);
    this.u32(id);
  }

  /** A condition (`Op::If`). */
  condition(b: boolean): void {
    this.u8(b ? 1 : 0);
  }

  /** A `u32` LE: a count (`Op::Loop`, arrays, objects) or a length. */
  u32(n: number): void {
    this.reserve(4);
    this.view.setUint32(this.length, n, true);
    this.length += 4;
  }

  f64(n: number): void {
    this.reserve(8);
    this.view.setFloat64(this.length, n, true);
    this.length += 8;
  }

  /** A string: `u32` LE byte length, then UTF-8. */
  string(s: string): void {
    // At most 3 bytes per UTF-16 unit.
    this.reserve(4 + s.length * 3);
    const start = this.length + 4;
    const buffer = this.buffer;
    // Most strings on a page are short and ASCII: written here, without a native call.
    let i = 0;
    if (s.length <= SHORT) {
      for (; i < s.length; i++) {
        const c = s.charCodeAt(i);
        if (c >= 0x80) break;
        buffer[start + i] = c;
      }
    }
    // Lone surrogates become U+FFFD, as `TextEncoder` writes them.
    const written = i === s.length ? i : buffer.write(s, start, "utf8");
    this.view.setUint32(this.length, written, true);
    this.length = start + written;
  }

  /** The bytes written. The input can't be written to after. */
  finish(): Uint8Array {
    nextSize = Math.min(Math.max(this.length, 4096), 1 << 20);
    return this.buffer.subarray(0, this.length);
  }
}

/** Strings this long or shorter are tried as ASCII first. */
const SHORT = 24;

/** A tagged primitive (`Op::Prop`): what React DOM writes as an attribute value. */
export function primitive(w: Input, v: unknown): void {
  switch (typeof v) {
    case "undefined":
      w.u8(TAG.undefined);
      return;
    case "boolean":
      w.u8(v ? TAG.true : TAG.false);
      return;
    case "number":
      w.u8(TAG.number);
      w.f64(v);
      return;
    case "string":
      w.u8(TAG.string);
      w.string(v);
      return;
    case "bigint":
      w.u8(TAG.bigint);
      w.string(v.toString());
      return;
    default:
      if (v === null) {
        w.u8(TAG.null);
        return;
      }
      throw new NotNative(`a ${typeof v} where a primitive goes`, v);
  }
}

function isPlainObject(v: unknown): v is Record<string, unknown> {
  if (v === null || typeof v !== "object") return false;
  const prototype: unknown = Object.getPrototypeOf(v);
  return prototype === Object.prototype || prototype === null;
}

/** A style value (`Op::Style`): `null`, `undefined`, or an object of primitives. */
export function style(w: Input, v: unknown): void {
  if (v === null || v === undefined) {
    primitive(w, v);
    return;
  }
  if (!isPlainObject(v)) throw new NotNative("a style that isn't a plain object", v);
  const entries = Object.entries(v);
  w.u8(TAG.object);
  w.u32(entries.length);
  for (const [name, item] of entries) {
    w.string(name);
    primitive(w, item);
  }
}

/** `Op::Value`: a value React's Flight server writes natively. */
export function value(w: Input, v: unknown): void {
  write(w, v, true);
}

/**
 * A value: with `identify`, each array and plain object with its identity first, as React writes
 * an object it has written before as a reference to it (ADR-0026).
 */
function write(w: Input, v: unknown, identify: boolean): void {
  if (Array.isArray(v)) {
    if (identify) w.identity(v);
    w.u8(TAG.array);
    w.u32(v.length);
    for (const item of v) write(w, item, identify);
  } else if (isPlainObject(v)) {
    if (identify) w.identity(v);
    const entries = Object.entries(v);
    w.u8(TAG.object);
    w.u32(entries.length);
    for (const [key, item] of entries) {
      w.string(key);
      write(w, item, identify);
    }
  } else if (v !== null && typeof v === "object") {
    throw new NotNative("an object that isn't plain or an array", v);
  } else {
    primitive(w, v);
  }
}

/** Matches the Flight references the engine writes as they are (`tsx2rs_ir::input::tag::REFERENCE`). */
const REFERENCE = /^\$[A-Za-z@]?[0-9a-f]+$/;

/**
 * A Flight reference to another row inside an `Op::Value`, such as a client component's type
 * (`$L15`, ADR-0020). React writes it unescaped, where a string value starting with `$` gets a
 * second `$`.
 */
export function reference(w: Input, ref: string): void {
  if (!REFERENCE.test(ref)) throw new Error(`not a Flight reference: ${JSON.stringify(ref)}`);
  w.u8(TAG.reference);
  w.string(ref);
}

/** What Next.js rendered for a client component element (ADR-0020), learned from its responses. */
export interface Island {
  /** Its Flight reference (`$L15`): the element's type. */
  readonly ref: string;
  /** Its HTML before its children (all of it, without children). */
  readonly before: string;
  /** Its HTML after its children. */
  readonly after: string;
}

/**
 * Resolves the `index`th client component of a route's encoder (in the order its manifest lists
 * them), rendered with `props` (without `children`), each time it's met in render order.
 */
export type Islands = (index: number, props: Record<string, unknown>) => Island;

/** A client component element's island, or {@link NotNative} when there's no resolver. */
export function island(
  islands: Islands | undefined,
  index: number,
  props: Record<string, unknown>,
): Island {
  if (islands === undefined)
    throw new NotNative("a client component element, not learned yet", props);
  return islands(index, props);
}

/**
 * What Next.js rendered for a `<Suspense>` boundary (ADR-0024), learned from its responses: the
 * boundary's reference (React's `Suspense` symbol row, `$b`), whether it was pending, and its
 * markers. Resolved: `<!--$-->`, then its children, then `<!--/$-->`. Pending: `<!--$?-->` and
 * its `<template id="B:…">`, its fallback, `<!--/$-->`; then, in the segments output, its
 * `<div hidden id="S:…">`, its children and `</div>` with React's reveal script.
 */
export interface Boundary {
  readonly ref: string;
  readonly pending: boolean;
  /** Before the fallback. */
  readonly open: string;
  /** After the fallback: the end of the boundary's part of the shell when it's pending. */
  readonly close: string;
  /** Before the children. */
  readonly segment: string;
  /** After the children. */
  readonly end: string;
}

/** Resolves each `<Suspense>` boundary a route renders, in render order. */
export type Boundaries = () => Boundary;

/** A `<Suspense>` boundary, or {@link NotNative} when there's no resolver. */
export function boundary(boundaries: Boundaries | undefined): Boundary {
  if (boundaries === undefined) throw new NotNative("a Suspense boundary, not learned yet", null);
  return boundaries();
}

/**
 * `Op::Island`: HTML as React wrote it. Its text-edge flags are 0: a learned island's HTML
 * includes the `<!-- -->` separators React wrote next to it.
 */
export function html(w: Input, markup: string): void {
  w.string(markup);
  w.u8(0);
}

/**
 * `Op::Attributes`: a props object built as JSX builds it (the data function has taken `key`
 * out, and an element with a `children` prop is an element value, not attributes).
 */
export function attrs(w: Input, props: unknown): void {
  if (!isPlainObject(props)) throw new NotNative("props that aren't a plain object", props);
  const entries = Object.entries(props);
  w.u32(entries.length);
  for (const [name, item] of entries) {
    w.string(name);
    if (name === "style") style(w, item);
    else primitive(w, item);
  }
}

const ELEMENT = Symbol.for("react.transitional.element");

interface Element {
  type: unknown;
  key: string | null;
  props: Record<string, unknown>;
}

function isElement(v: unknown): v is Element {
  return v !== null && typeof v === "object" && (v as { $$typeof?: unknown }).$$typeof === ELEMENT;
}

/** A component element's type as a name for reasons: `Suspense`, `ProductGallery`, … */
function componentName(type: unknown): string {
  if (typeof type === "function") {
    const named = type as { displayName?: unknown; name?: unknown };
    if (typeof named.displayName === "string" && named.displayName !== "") return named.displayName;
    if (typeof named.name === "string" && named.name !== "") return named.name;
    return "anonymous";
  }
  if (typeof type === "symbol") return (type.description ?? "").replace(/^react\./, "") || "symbol";
  if (type !== null && typeof type === "object") {
    // `memo`, `forwardRef`, `lazy`, context providers: React's own objects.
    const tag = (type as { $$typeof?: unknown }).$$typeof;
    return typeof tag === "symbol"
      ? (tag.description ?? "object").replace(/^react\./, "")
      : "object";
  }
  return typeof type;
}

/** `Op::Child`: a React node. Host elements are element nodes (ADR-0007). */
export function node(w: Input, v: unknown): void {
  if (Array.isArray(v)) {
    w.u8(TAG.array);
    w.u32(v.length);
    for (const item of v) node(w, item);
  } else if (isElement(v)) {
    if (typeof v.type !== "string") {
      throw new NotNative(
        `a component element (<${componentName(v.type)}>), rendered by React for now`,
        v,
      );
    }
    w.u8(TAG.element);
    w.string(v.type);
    primitive(w, v.key);
    // Props in order; `children` is a marker there, and its node follows the last prop.
    const entries = Object.entries(v.props);
    w.u32(entries.length);
    for (const [name, item] of entries) {
      w.string(name);
      // An element's props are read by the engine's element writer, which takes no identities.
      if (name !== "children") write(w, item, false);
    }
    if ("children" in v.props) node(w, v.props["children"]);
  } else if (v !== null && typeof v === "object") {
    // A promise, an iterable that isn't an array, a lazy value, or an object React rejects.
    throw new NotNative("a node React renders itself", v);
  } else if (typeof v === "function" || typeof v === "symbol") {
    throw new NotNative(`a ${typeof v} as a child`, v);
  } else {
    primitive(w, v);
  }
}

/** React's key path state for a component's output (ADR-0008). */
export interface Keys {
  /** The keys of the component elements above, joined with `,`. */
  readonly path: string | null;
  /** Whether a keyed root element is wrapped in an array (React's implicit slot). */
  readonly implicit: boolean;
}

/** A page: Next.js renders it as an unkeyed function component. */
export const ENTRY_KEYS: Keys = { path: null, implicit: true };
/** Inside an element: key paths don't reach this far. */
export const NO_KEYS: Keys = { path: null, implicit: false };

/** A root element's Flight key: its own key with the key path in front. */
export function rootKey(keys: Keys, own: unknown): string | null {
  const key = own as string | null;
  if (keys.path === null) return key;
  return key === null ? keys.path : `${keys.path},${key}`;
}

/** A key as React makes it: `"" + key`, so an object key is `"[object Object]"` there too. */
function keyString(key: unknown): string {
  return String(key);
}

/** A template's encoder: writes `record` (its data function's) with the key path `keys`. */
export type Body = (w: Input, record: unknown[], keys: Keys) => void;
/** A compiled component's data function. */
export type DataFunction = (props: Record<string, unknown>) => unknown[];

/**
 * A pending call `[config, key]` (ADR-0013), resolved as React's `jsx(type, config, key)` does,
 * then the callee's data function run with its props and its record written by `body`. `outer`
 * is the key path where the component element is.
 */
export function call(
  w: Input,
  outer: Keys,
  pending: unknown,
  data: DataFunction,
  body: Body,
): void {
  const { props, keys } = callee(outer, pending);
  body(w, data(props), keys);
}

/**
 * {@link call} for an async server component (ADR-0025): its data function returns a promise, and
 * the encoder that calls it is async.
 */
export async function callAsync(
  w: Input,
  outer: Keys,
  pending: unknown,
  data: (props: unknown) => unknown,
  body: (w: Input, record: unknown, keys: Keys) => unknown,
): Promise<void> {
  const { props, keys } = callee(outer, pending);
  await body(w, await data(props), keys);
}

/** A pending call's props and key path, as React's `jsx` takes them from its arguments. */
function callee(outer: Keys, pending: unknown): { props: Record<string, unknown>; keys: Keys } {
  const [config, maybeKey] = pending as [Record<string, unknown>, unknown];
  let key = maybeKey === undefined ? null : keyString(maybeKey);
  const props: Record<string, unknown> = {};
  for (const [name, item] of Object.entries(config)) {
    if (name === "key") {
      if (item !== undefined) key = keyString(item);
    } else props[name] = item;
  }
  const keys: Keys =
    key === null
      ? { path: outer.path, implicit: outer.path === null ? true : outer.implicit }
      : { path: outer.path === null ? key : `${outer.path},${key}`, implicit: outer.implicit };
  return { props, keys };
}

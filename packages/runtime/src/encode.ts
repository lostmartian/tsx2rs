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

const encoder = new TextEncoder();

/** The engine's input, written in program order. */
export class Input {
  private buffer = new Uint8Array(256);
  private view = new DataView(this.buffer.buffer);
  private length = 0;

  private reserve(n: number): void {
    if (this.length + n <= this.buffer.length) return;
    let size = this.buffer.length * 2;
    while (size < this.length + n) size *= 2;
    const next = new Uint8Array(size);
    next.set(this.buffer.subarray(0, this.length));
    this.buffer = next;
    this.view = new DataView(next.buffer);
  }

  u8(n: number): void {
    this.reserve(1);
    this.buffer[this.length++] = n;
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
    const { written } = encoder.encodeInto(s, this.buffer.subarray(this.length + 4));
    this.view.setUint32(this.length, written, true);
    this.length += 4 + written;
  }

  /** The bytes written. */
  finish(): Uint8Array {
    return this.buffer.slice(0, this.length);
  }
}

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
  if (Array.isArray(v)) {
    w.u8(TAG.array);
    w.u32(v.length);
    for (const item of v) value(w, item);
  } else if (isPlainObject(v)) {
    const entries = Object.entries(v);
    w.u8(TAG.object);
    w.u32(entries.length);
    for (const [key, item] of entries) {
      w.string(key);
      value(w, item);
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

/** `Op::Child`: a React node. Host elements are element nodes (ADR-0007). */
export function node(w: Input, v: unknown): void {
  if (Array.isArray(v)) {
    w.u8(TAG.array);
    w.u32(v.length);
    for (const item of v) node(w, item);
  } else if (isElement(v)) {
    if (typeof v.type !== "string") {
      throw new NotNative("a component element, rendered by React for now", v);
    }
    w.u8(TAG.element);
    w.string(v.type);
    primitive(w, v.key);
    // Props in order; `children` is a marker there, and its node follows the last prop.
    const entries = Object.entries(v.props);
    w.u32(entries.length);
    for (const [name, item] of entries) {
      w.string(name);
      if (name !== "children") value(w, item);
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
  body(w, data(props), keys);
}

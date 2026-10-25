/**
 * Extracts and decodes the React Flight (RSC) payload that Next.js inlines in an HTML document,
 * into one tree with every reference resolved, so two payloads can be compared by meaning rather
 * than by how React happened to split them into rows (D-009, ADR-0002).
 *
 * The row grammar and value encodings follow React 19.3.1's Flight client
 * (`react-server-dom-turbopack-client.node.production.js`: `processBinaryChunk`,
 * `processFullStringRow`, `parseModelString`, `getOutlinedModel`). Anything outside what is
 * modelled here throws an `IdentityError` instead of being approximated.
 */
import { IdentityError } from "./errors.ts";

// ---------------------------------------------------------------- extraction from HTML

/** `<script>self.__next_f.push([kind, data])</script>`, as Next.js writes it (optional nonce). */
const NEXT_F_PUSH =
  /<script(?:\s+nonce="[^"]*")?>self\.__next_f\.push\((\[(?:\d+)(?:,(?:"(?:\\.|[^"\\])*"|null))?\])\)<\/script>/g;

export interface ExtractedFlight {
  /** The concatenated Flight bytes, in document order. */
  readonly payload: Uint8Array;
  /** The document with the payload scripts removed (bootstrap and form-state scripts stay). */
  readonly html: string;
}

/**
 * Splits a Next.js HTML document into its Flight payload and the rest of the HTML.
 * Kind 1 pushes carry text, kind 3 pushes carry base64 bytes; kinds 0 (bootstrap) and
 * 2 (form state) are part of the document and stay in the HTML.
 */
export function extractFlight(html: string): ExtractedFlight {
  const parts: Uint8Array[] = [];
  const encoder = new TextEncoder();
  const rest = html.replace(NEXT_F_PUSH, (script, arrayText: string) => {
    const [kind, data] = JSON.parse(arrayText) as [number, string | null | undefined];
    if (kind === 1 && typeof data === "string") {
      parts.push(encoder.encode(data));
      return "";
    }
    if (kind === 3 && typeof data === "string") {
      parts.push(Uint8Array.from(Buffer.from(data, "base64")));
      return "";
    }
    return script;
  });
  return { payload: concat(parts), html: rest };
}

function concat(parts: readonly Uint8Array[]): Uint8Array {
  const out = new Uint8Array(parts.reduce((n, p) => n + p.length, 0));
  let offset = 0;
  for (const p of parts) {
    out.set(p, offset);
    offset += p.length;
  }
  return out;
}

// ---------------------------------------------------------------- rows

export type Row =
  | { readonly kind: "model"; readonly json: string }
  | { readonly kind: "module"; readonly json: string }
  | { readonly kind: "text"; readonly text: string }
  | { readonly kind: "error"; readonly json: string }
  | {
      readonly kind: "stream";
      readonly stream: "readable" | "asyncIterable" | "asyncIterator";
      readonly items: string[];
      close?: string;
    };

export interface Rows {
  /** Rows by id (lowercase hex, as written). */
  readonly byId: ReadonlyMap<string, Row>;
  /** Hint rows (`:H…`, preload/preconnect instructions), in order, as `[code, json]`. */
  readonly hints: readonly (readonly [string, string])[];
}

const LENGTH_PREFIXED_TAGS = new Set("TAOobUSsLlGgMmV");
const DEVELOPMENT_TAGS = new Set("DNJW");
const STREAM_TAGS: Readonly<Record<string, "readable" | "asyncIterable" | "asyncIterator">> = {
  R: "readable",
  X: "asyncIterable",
  x: "asyncIterator",
};
const NEWLINE = 0x0a;
const COLON = 0x3a;
const COMMA = 0x2c;

/** Parses a Flight payload into rows (React's `processBinaryChunk` grammar). */
export function parseRows(payload: Uint8Array): Rows {
  const decoder = new TextDecoder("utf-8", { fatal: true });
  const text = (from: number, to: number) => decoder.decode(payload.subarray(from, to));
  const byId = new Map<string, Row>();
  const hints: (readonly [string, string])[] = [];

  let i = 0;
  while (i < payload.length) {
    const colon = payload.indexOf(COLON, i);
    if (colon === -1)
      throw new IdentityError("MALFORMED_FLIGHT", `row without ':' at byte ${String(i)}`);
    const id = text(i, colon);
    if (!/^[0-9a-f]*$/.test(id)) {
      throw new IdentityError("MALFORMED_FLIGHT", `row id ${JSON.stringify(id)} is not hex`);
    }
    const tag = String.fromCharCode(payload[colon + 1] ?? NEWLINE);

    if (LENGTH_PREFIXED_TAGS.has(tag)) {
      const comma = payload.indexOf(COMMA, colon + 2);
      const length = Number.parseInt(text(colon + 2, comma), 16);
      if (comma === -1 || Number.isNaN(length)) {
        throw new IdentityError("MALFORMED_FLIGHT", `row ${id}: bad length prefix`);
      }
      const end = comma + 1 + length;
      if (end > payload.length) throw new IdentityError("MALFORMED_FLIGHT", `row ${id}: truncated`);
      if (tag !== "T") {
        throw new IdentityError("UNSUPPORTED_FLIGHT_ROW", `row ${id}: binary row type ${tag}`);
      }
      setRow(byId, id, { kind: "text", text: text(comma + 1, end) });
      i = end;
      continue;
    }

    let lineEnd = payload.indexOf(NEWLINE, colon + 1);
    if (lineEnd === -1) lineEnd = payload.length;
    const isLineTag = /^[A-Z#rx]$/.test(tag);
    const body = text(colon + 1 + (isLineTag ? 1 : 0), lineEnd);
    i = lineEnd + 1;

    if (!isLineTag) {
      appendModel(byId, id, body);
    } else if (tag === "I") {
      setRow(byId, id, { kind: "module", json: body });
    } else if (tag === "H") {
      hints.push([body.slice(0, 1), body.slice(1)]);
    } else if (tag === "E") {
      setRow(byId, id, { kind: "error", json: body });
    } else if (tag in STREAM_TAGS) {
      const stream = STREAM_TAGS[tag] ?? "readable";
      setRow(byId, id, { kind: "stream", stream, items: [] });
    } else if (tag === "C") {
      const row = byId.get(id);
      if (row?.kind !== "stream" || row.close !== undefined) {
        throw new IdentityError("MALFORMED_FLIGHT", `row ${id}: close without an open stream`);
      }
      row.close = body;
    } else if (DEVELOPMENT_TAGS.has(tag)) {
      throw new IdentityError(
        "UNSUPPORTED_FLIGHT_ROW",
        `row ${id}: development-only row type ${tag} (compare production builds)`,
      );
    } else {
      throw new IdentityError("UNSUPPORTED_FLIGHT_ROW", `row ${id}: row type ${tag}`);
    }
  }
  return { byId, hints };
}

function setRow(byId: Map<string, Row>, id: string, row: Row): void {
  if (byId.has(id)) throw new IdentityError("MALFORMED_FLIGHT", `duplicate row id ${id}`);
  byId.set(id, row);
}

/** A plain JSON row: a model, or the next item of an open stream with the same id. */
function appendModel(byId: Map<string, Row>, id: string, json: string): void {
  const existing = byId.get(id);
  if (existing?.kind === "stream" && existing.close === undefined) {
    existing.items.push(json);
    return;
  }
  setRow(byId, id, { kind: "model", json });
}

// ---------------------------------------------------------------- decoding

/**
 * A decoded value. Plain JSON stays plain; everything React gives special meaning is wrapped as
 * `{ $$identity: kind, value }` so it can't collide with ordinary data.
 */
export type Decoded =
  null | boolean | number | string | Decoded[] | { [key: string]: Decoded } | Tagged;

export interface Tagged {
  readonly $$identity:
    | "element"
    | "symbol"
    | "undefined"
    | "number"
    | "date"
    | "bigint"
    | "promise"
    | "map"
    | "set"
    | "clientReference"
    | "serverReference"
    | "error"
    | "stream";
  readonly value: Decoded;
}

export interface DecodedFlight {
  /** Row 0, fully resolved. */
  readonly root: Decoded;
  /** Hint rows, in order. */
  readonly hints: readonly (readonly [string, Decoded])[];
}

const tagged = (kind: Tagged["$$identity"], value: Decoded): Tagged => ({
  $$identity: kind,
  value,
});

/** Decodes a Flight payload into one tree, starting from row 0. */
export function decodeFlight(payload: Uint8Array): DecodedFlight {
  const rows = parseRows(payload);
  const decoder = new FlightDecoder(rows.byId);
  return {
    root: decoder.row("0", []),
    hints: rows.hints.map(([code, json]) => [code, decoder.json(json, ["(hint)"])] as const),
  };
}

class FlightDecoder {
  /** `row:path` nodes being decoded right now; meeting one again is a real cycle. */
  private readonly active = new Set<string>();
  /** Parsed JSON of model rows, by id. */
  private readonly parsed = new Map<string, unknown>();

  private readonly rows: ReadonlyMap<string, Row>;

  constructor(rows: ReadonlyMap<string, Row>) {
    this.rows = rows;
  }

  /** Row `id`, fully decoded. */
  row(id: string, path: readonly string[]): Decoded {
    return this.node(id, [], path);
  }

  json(json: string, path: readonly string[]): Decoded {
    return this.value(this.parse(json, path), path);
  }

  /** The value at `segments` inside row `id`, decoded. */
  private node(id: string, segments: readonly string[], path: readonly string[]): Decoded {
    const key = [id, ...segments].join(":");
    if (this.active.has(key)) {
      throw new IdentityError("CYCLIC_FLIGHT_REFERENCE", `${key} (at ${describe(path)})`);
    }
    this.active.add(key);
    try {
      if (segments.length > 0) return this.value(this.walk(id, segments, path), path);
      const row = this.lookup(id, path);
      switch (row.kind) {
        case "model":
          return this.value(this.parsedRow(id, row, path), path);
        case "text":
          return row.text;
        case "module":
          return tagged("clientReference", this.json(row.json, path));
        case "error":
          return tagged("error", this.json(row.json, path));
        case "stream":
          return tagged("stream", {
            type: row.stream,
            items: row.items.map((item, n) => this.json(item, [...path, `item ${String(n)}`])),
            close:
              row.close === undefined
                ? tagged("undefined", null)
                : this.closeValue(row.close, path),
          });
      }
    } finally {
      this.active.delete(key);
    }
  }

  private lookup(id: string, path: readonly string[]): Row {
    const row = this.rows.get(id);
    if (row === undefined) {
      throw new IdentityError("DANGLING_FLIGHT_REFERENCE", `row ${id} (at ${describe(path)})`);
    }
    return row;
  }

  private parsedRow(id: string, row: Row & { kind: "model" }, path: readonly string[]): unknown {
    if (!this.parsed.has(id)) this.parsed.set(id, this.parse(row.json, path));
    return this.parsed.get(id);
  }

  /**
   * Walks the raw JSON of row `id` along `segments`, the way React's `getOutlinedModel` walks
   * the revived value: references met on the way are followed (lazy ones too), and an element
   * `["$", type, key, props]` exposes `type`, `key` and `props`.
   */
  private walk(id: string, segments: readonly string[], path: readonly string[]): unknown {
    const row = this.lookup(id, path);
    if (row.kind !== "model") {
      throw new IdentityError(
        "MALFORMED_FLIGHT",
        `path into ${row.kind} row ${id} (at ${describe(path)})`,
      );
    }
    let raw = this.parsedRow(id, row, path);
    for (const segment of segments) {
      raw = this.follow(raw, path);
      const index = Array.isArray(raw) && raw[0] === "$" ? ELEMENT_FIELDS[segment] : segment;
      const next =
        raw !== null && typeof raw === "object" && index !== undefined
          ? (raw as Record<string, unknown>)[index]
          : undefined;
      if (next === undefined) {
        throw new IdentityError(
          "DANGLING_FLIGHT_REFERENCE",
          `path ${[id, ...segments].join(":")} (at ${describe(path)})`,
        );
      }
      raw = next;
    }
    return raw;
  }

  /** If `raw` is a reference string (`$<id>[:path]` or `$L<id>`), the raw value it points to. */
  private follow(raw: unknown, path: readonly string[]): unknown {
    while (typeof raw === "string") {
      const m = /^\$L?([0-9a-f]+)((?::[^:]+)*)$/.exec(raw);
      if (m === null) return raw;
      const [, id = "", rest = ""] = m;
      const segments = rest === "" ? [] : rest.slice(1).split(":");
      raw = segments.length === 0 ? this.rawRow(id, path) : this.walk(id, segments, path);
    }
    return raw;
  }

  private rawRow(id: string, path: readonly string[]): unknown {
    const row = this.lookup(id, path);
    if (row.kind !== "model") {
      throw new IdentityError(
        "MALFORMED_FLIGHT",
        `path through ${row.kind} row ${id} (at ${describe(path)})`,
      );
    }
    return this.parsedRow(id, row, path);
  }

  private closeValue(body: string, path: readonly string[]): Decoded {
    // React: `close("" === row ? '"$undefined"' : row)`
    return body === "" ? tagged("undefined", null) : this.json(body, path);
  }

  private parse(json: string, path: readonly string[]): unknown {
    try {
      return JSON.parse(json) as unknown;
    } catch {
      throw new IdentityError("MALFORMED_FLIGHT", `invalid JSON at ${describe(path)}`);
    }
  }

  private value(v: unknown, path: readonly string[]): Decoded {
    if (typeof v === "string") return this.string(v, path);
    if (Array.isArray(v)) {
      if (v[0] === "$" && v.length >= 4) {
        const [, type, key, props] = v as [string, unknown, unknown, unknown];
        return tagged("element", {
          type: this.value(type, [...path, "type"]),
          key: this.value(key, [...path, "key"]),
          props: this.value(props, [...path, "props"]),
        });
      }
      return v.map((item: unknown, n) => this.value(item, [...path, String(n)]));
    }
    if (v !== null && typeof v === "object") {
      // Object.fromEntries defines own properties, so a "__proto__" key stays data.
      return Object.fromEntries(
        Object.entries(v).map(([k, item]) => [k, this.value(item, [...path, k])]),
      );
    }
    // JSON.parse produces nothing else: strings, arrays and objects are handled above.
    return v as null | boolean | number;
  }

  /** React's `parseModelString`. */
  private string(s: string, path: readonly string[]): Decoded {
    if (!s.startsWith("$")) return s;
    if (s === "$") {
      throw new IdentityError(
        "MALFORMED_FLIGHT",
        `element marker outside an element at ${describe(path)}`,
      );
    }
    const kind = s.charAt(1);
    const rest = s.slice(2);
    switch (kind) {
      case "$":
        return s.slice(1);
      case "L":
        return this.reference(rest, path); // lazy: transparent once resolved
      case "@":
        return tagged("promise", this.reference(rest, path));
      case "S":
        return tagged("symbol", rest);
      case "h":
        return tagged("serverReference", this.reference(rest, path));
      case "Q":
        return tagged("map", this.reference(rest, path));
      case "W":
        return tagged("set", this.reference(rest, path));
      case "Z":
        return tagged("error", null);
      case "I":
        return tagged("number", "Infinity");
      case "-":
        return tagged("number", s === "$-0" ? "-0" : "-Infinity");
      case "N":
        return tagged("number", "NaN");
      case "u":
        return tagged("undefined", null);
      case "D":
        return tagged("date", rest);
      case "n":
        return tagged("bigint", rest);
      default:
        if (/^[0-9a-f]/.test(kind)) return this.reference(s.slice(1), path);
        throw new IdentityError(
          "UNSUPPORTED_FLIGHT_VALUE",
          `${JSON.stringify(s.slice(0, 2))} at ${describe(path)}`,
        );
    }
  }

  /** `id` or `id:path:segments` (React's deduplication of already-sent objects). */
  private reference(reference: string, path: readonly string[]): Decoded {
    const [id = "", ...segments] = reference.split(":");
    if (!/^[0-9a-f]+$/.test(id)) {
      throw new IdentityError("MALFORMED_FLIGHT", `bad reference ${JSON.stringify(reference)}`);
    }
    return this.node(id, segments, path);
  }
}

/** Element tuple positions: `["$", type, key, props]`. */
const ELEMENT_FIELDS: Readonly<Record<string, number | undefined>> = { type: 1, key: 2, props: 3 };

function describe(path: readonly string[]): string {
  return path.length === 0 ? "root" : path.join(".");
}

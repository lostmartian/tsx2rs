/**
 * The identity check (ADR-0002): two server-rendered documents are identical when
 * 1. their HTML, after applying streamed boundaries and removing the Flight scripts, is
 *    byte-identical, and
 * 2. their Flight payloads decode to deep-equal trees (with hints, in order).
 */
import { decodeFlight, extractFlight, type Decoded, type DecodedFlight } from "./flight.ts";
import { resolveStreamedHtml } from "./streaming.ts";

/** Where two HTML strings first differ. */
export interface HtmlDifference {
  readonly offset: number;
  readonly line: number;
  readonly column: number;
  readonly expected: string;
  readonly actual: string;
}

/** Where two decoded Flight trees first differ. */
export interface FlightDifference {
  /** Path from the root, e.g. `root.value.props.children.1`. */
  readonly path: string;
  readonly expected: unknown;
  readonly actual: unknown;
}

export interface IdentityReport {
  readonly identical: boolean;
  readonly html: {
    readonly identical: boolean;
    readonly length: number;
    readonly difference?: HtmlDifference;
  };
  readonly flight: { readonly identical: boolean; readonly difference?: FlightDifference };
  /** Streamed boundaries/segments applied, per side (informational: these may legitimately differ). */
  readonly streamed: { readonly expected: number; readonly actual: number };
}

export interface NormalizedDocument {
  readonly html: string;
  readonly flight: DecodedFlight;
  readonly streamed: number;
}

/** A document as the browser ends up with it: streams applied, Flight decoded. */
export function normalizeDocument(html: string): NormalizedDocument {
  const { payload, html: withoutFlight } = extractFlight(html);
  const resolved = resolveStreamedHtml(withoutFlight);
  return { html: resolved.html, flight: decodeFlight(payload), streamed: resolved.applied.length };
}

/** Compares `actual` (e.g. tsx2rs) with `expected` (Next.js). */
export function compareDocuments(expected: string, actual: string): IdentityReport {
  const a = normalizeDocument(expected);
  const b = normalizeDocument(actual);
  const htmlDifference = firstHtmlDifference(a.html, b.html);
  const flightDifference = firstTreeDifference(
    { root: a.flight.root, hints: a.flight.hints as unknown as Decoded },
    { root: b.flight.root, hints: b.flight.hints as unknown as Decoded },
    [],
  );
  return {
    identical: htmlDifference === undefined && flightDifference === undefined,
    html: {
      identical: htmlDifference === undefined,
      length: a.html.length,
      ...(htmlDifference && { difference: htmlDifference }),
    },
    flight: {
      identical: flightDifference === undefined,
      ...(flightDifference && { difference: flightDifference }),
    },
    streamed: { expected: a.streamed, actual: b.streamed },
  };
}

const CONTEXT = 60;

export function firstHtmlDifference(expected: string, actual: string): HtmlDifference | undefined {
  if (expected === actual) return undefined;
  let offset = 0;
  while (
    offset < expected.length &&
    offset < actual.length &&
    expected[offset] === actual[offset]
  ) {
    offset++;
  }
  const before = expected.slice(0, offset);
  const line = before.split("\n").length;
  const column = offset - before.lastIndexOf("\n");
  const from = Math.max(0, offset - CONTEXT);
  return {
    offset,
    line,
    column,
    expected: expected.slice(from, offset + CONTEXT),
    actual: actual.slice(from, offset + CONTEXT),
  };
}

export function firstTreeDifference(
  expected: unknown,
  actual: unknown,
  path: readonly string[],
): FlightDifference | undefined {
  const here = (): FlightDifference => ({ path: path.join(".") || "(root)", expected, actual });
  if (Object.is(expected, actual)) return undefined;
  if (
    typeof expected !== "object" ||
    typeof actual !== "object" ||
    expected === null ||
    actual === null
  ) {
    return here();
  }
  if (Array.isArray(expected) !== Array.isArray(actual)) return here();
  if (Array.isArray(expected) && Array.isArray(actual)) {
    for (let i = 0; i < Math.max(expected.length, actual.length); i++) {
      if (i >= expected.length || i >= actual.length) {
        return { path: [...path, String(i)].join("."), expected: expected[i], actual: actual[i] };
      }
      const d = firstTreeDifference(expected[i], actual[i], [...path, String(i)]);
      if (d) return d;
    }
    return undefined;
  }
  const e = expected as Record<string, unknown>;
  const a = actual as Record<string, unknown>;
  const keys = [...new Set([...Object.keys(e), ...Object.keys(a)])];
  for (const key of keys) {
    if (!(key in e) || !(key in a)) {
      return { path: [...path, key].join("."), expected: e[key], actual: a[key] };
    }
    const d = firstTreeDifference(e[key], a[key], [...path, key]);
    if (d) return d;
  }
  // Same keys in a different order are equal as data, but JSON consumers may not agree.
  if (Object.keys(e).join("\0") !== Object.keys(a).join("\0")) {
    return {
      path: `${path.join(".") || "(root)"} (key order)`,
      expected: Object.keys(e),
      actual: Object.keys(a),
    };
  }
  return undefined;
}

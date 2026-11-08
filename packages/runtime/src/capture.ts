/**
 * Capturing Next.js's wrapper around a compiled page from real responses (ADR-0018).
 *
 * A route the runtime can't serve yet goes to Next.js, as fallback does. From such a response
 * the runtime **learns** a template: Next.js's document with every fragment the runtime renders
 * per request (the page's HTML and its Flight row) found verbatim, exactly once, and replaced by
 * a slot. Each place a route param's value appears is a **candidate** slot. A second response
 * with different params **confirms** it: every byte must match, a candidate becomes a slot only
 * if the other response has its own value there, and anything else that differs refuses the
 * route. From then on the runtime **fills** the template per request.
 *
 * Next.js writes the Flight payload into the document as `<script>self.__next_f.push([1,"…"])`
 * scripts, encoded with `htmlEscapeJsonString(JSON.stringify(…))`. A template keeps each script
 * as a unit, decoded, and fills it by encoding it the same way; a document whose scripts don't
 * round-trip that way is refused.
 */

/** A response the runtime saw, and what it rendered for it. */
export interface Sample {
  /** Next.js's response body: the whole HTML document. */
  document: string;
  /** The route's params for the request, as Next.js gives them to the page (not decoded). */
  params: Record<string, string>;
  /** The request's path, as requested (`/q/3`). */
  pathname: string;
  /** The request's query string, `?` included, or `""`. */
  search: string;
  /** What the runtime renders per request, found in the document verbatim. */
  fragments: Fragment[];
}

/** A part of the response the runtime renders itself. */
export interface Fragment {
  name: string;
  /** Its HTML, found once in the document outside the Flight scripts. */
  html: string;
  /** Its Flight row's value (after `id:`), found once among the Flight rows. */
  flight: string;
}

/** What a template is filled with. */
export interface Values {
  params: Record<string, string>;
  pathname: string;
  search: string;
  fragments: Record<string, { html: string; flight: string }>;
}

/** How a param's value is written where it appears. */
export type Encoding = "html" | "html-uri" | "json" | "json-uri";

type Part =
  | { text: string }
  | { fragment: string; as: "html" | "flight" }
  | { param: string; encoding: Encoding }
  /** A value that may be a param, until a response with another value says. */
  | { candidate: string; encoding: Encoding; literal: string }
  /** The URL in Next.js's router state: `"c"` (path and query, split on `/`) or `"q"` (query). */
  | { url: "c" | "q" };

type Unit = { kind: "html"; parts: Part[] } | { kind: "flight"; start: string; parts: Part[] };

/** A learned wrapper. */
export interface Template {
  units: Unit[];
  /** The params' values it was learned with. */
  learned: Record<string, string>;
}

export type Learned = { ok: true; template: Template } | { ok: false; reason: string };

const PUSH = /(<script(?: nonce="[^"]*")?>)self\.__next_f\.push\(([\s\S]*?)\)<\/script>/g;

const ESCAPES: Record<string, string> = {
  "&": "\\u0026",
  ">": "\\u003e",
  "<": "\\u003c",
  "\u2028": "\\u2028",
  "\u2029": "\\u2029",
};

/** Next.js's `htmlEscapeJsonString`. */
function htmlEscapeJson(s: string): string {
  return s.replace(/[&><\u2028\u2029]/g, (c) => ESCAPES[c] ?? c);
}

/** A Flight data chunk as Next.js writes it into the document. */
function flightScript(start: string, chunk: string): string {
  return `${start}self.__next_f.push(${htmlEscapeJson(JSON.stringify([1, chunk]))})</script>`;
}

/** React DOM's escaping of text and attribute values. */
function htmlEscape(s: string): string {
  return s.replace(/[&<>"']/g, (c) =>
    c === "&" ? "&amp;" : c === "<" ? "&lt;" : c === ">" ? "&gt;" : c === '"' ? "&quot;" : "&#x27;",
  );
}

function encode(value: string, encoding: Encoding): string {
  switch (encoding) {
    case "html":
      return htmlEscape(value);
    case "html-uri":
      return htmlEscape(encodeURIComponent(value));
    case "json":
      return JSON.stringify(value).slice(1, -1);
    case "json-uri":
      return JSON.stringify(encodeURIComponent(value)).slice(1, -1);
  }
}

const ENCODINGS: Record<Unit["kind"], Encoding[]> = {
  html: ["html", "html-uri"],
  flight: ["json", "json-uri"],
};

class Refused extends Error {}

/** The document as text between Flight scripts, and the scripts' chunks, decoded. */
type Parsed = ({ kind: "html"; text: string } | { kind: "flight"; start: string; text: string })[];

function parse(document: string): Parsed {
  const out: Parsed = [];
  let at = 0;
  for (const m of document.matchAll(PUSH)) {
    const [whole, start = "", payload = ""] = m;
    out.push({ kind: "html", text: document.slice(at, m.index) });
    let decoded: unknown;
    try {
      decoded = JSON.parse(payload);
    } catch {
      throw new Refused("a Flight script that isn't JSON");
    }
    if (!Array.isArray(decoded) || decoded[0] !== 1 || typeof decoded[1] !== "string") {
      throw new Refused("a Flight script that isn't a text chunk (binary Flight data)");
    }
    if (flightScript(start, decoded[1]) !== whole) {
      throw new Refused("a Flight script not encoded as Next.js 16.4 encodes them");
    }
    out.push({ kind: "flight", start, text: decoded[1] });
    at = m.index + whole.length;
  }
  out.push({ kind: "html", text: document.slice(at) });
  return out;
}

/**
 * The document as text between Flight scripts, and the scripts' chunks, decoded; empty for a
 * document whose scripts Next.js 16.4 didn't write.
 */
export function decode(document: string): Parsed {
  try {
    return parse(document);
  } catch (error) {
    if (error instanceof Refused) return [];
    throw error;
  }
}

/** Every occurrence of `needle` in `parts`' text, as [part index, offset]. */
function occurrences(parts: Part[], needle: string): [number, number][] {
  const found: [number, number][] = [];
  parts.forEach((part, i) => {
    if (!("text" in part) || needle === "") return;
    for (let at = part.text.indexOf(needle); at !== -1; at = part.text.indexOf(needle, at + 1)) {
      found.push([i, at]);
    }
  });
  return found;
}

/** `parts` with the text at [part, offset, length] replaced by `slot`. */
function cut(parts: Part[], [i, at]: [number, number], length: number, slot: Part): Part[] {
  const part = parts[i];
  if (part === undefined || !("text" in part)) return parts;
  const replaced: Part[] = [
    { text: part.text.slice(0, at) },
    slot,
    { text: part.text.slice(at + length) },
  ];
  return [
    ...parts.slice(0, i),
    ...replaced.filter((p) => !("text" in p) || p.text !== ""),
    ...parts.slice(i + 1),
  ];
}

/** Learns a template from one response. */
export function learn(sample: Sample): Learned {
  try {
    const units: Unit[] = parse(sample.document).map((u) =>
      u.kind === "html"
        ? { kind: "html", parts: [{ text: u.text }] }
        : { kind: "flight", start: u.start, parts: [{ text: u.text }] },
    );
    for (const fragment of sample.fragments) {
      // A fragment may have only HTML (a page's segments, ADR-0024).
      if (fragment.html !== "") {
        place(units, "html", fragment.html, { fragment: fragment.name, as: "html" });
      }
      // A row's value: after its `id:`, up to its newline.
      if (fragment.flight !== "") {
        place(
          units,
          "flight",
          fragment.flight,
          { fragment: fragment.name, as: "flight" },
          ":",
          "\n",
        );
      }
    }
    // The URL is in the router state twice (Next.js 16.4's initial payload, row 0).
    place(units, "flight", url("c", sample), { url: "c" });
    place(units, "flight", url("q", sample), { url: "q" });
    for (const [name, value] of Object.entries(sample.params)) {
      for (const unit of units) {
        for (const encoding of ENCODINGS[unit.kind]) {
          const literal = encode(value, encoding);
          // Last first, so earlier offsets stay valid.
          for (const at of occurrences(unit.parts, literal).reverse()) {
            unit.parts = cut(unit.parts, at, literal.length, {
              candidate: name,
              encoding,
              literal,
            });
          }
        }
      }
    }
    return { ok: true, template: { units, learned: { ...sample.params } } };
  } catch (error) {
    if (error instanceof Refused) return { ok: false, reason: error.message };
    throw error;
  }
}

/** Replaces the one occurrence of `text` (between `before` and `after`) in units of `kind`. */
function place(
  units: Unit[],
  kind: Unit["kind"],
  text: string,
  slot: Part,
  before = "",
  after = "",
) {
  const needle = `${before}${text}${after}`;
  const found = units.flatMap((unit, u) =>
    unit.kind === kind ? occurrences(unit.parts, needle).map((at) => [u, at] as const) : [],
  );
  const what =
    "fragment" in slot
      ? `${slot.fragment}'s ${slot.as}`
      : `the router state's URL ("${"url" in slot ? slot.url : ""}")`;
  if (found.length !== 1) {
    throw new Refused(
      `${what} found ${String(found.length)} times in Next.js's response, not once`,
    );
  }
  const [[u, [i, at]]] = found as [[number, [number, number]]];
  const unit = units[u];
  if (unit === undefined) return;
  unit.parts = cut(unit.parts, [i, at + before.length], text.length, slot);
}

/**
 * Confirms a template with another response: every byte must match. A candidate is a slot if
 * that response has its own param value there, literal if it has the learned one; where the two
 * values are equal it stays a candidate.
 */
export function confirm(template: Template, sample: Sample): Learned {
  try {
    const parsed = parse(sample.document);
    if (parsed.length !== template.units.length) {
      throw new Refused(
        "Next.js's response has another structure (a different number of Flight scripts)",
      );
    }
    const values: Values = {
      params: sample.params,
      pathname: sample.pathname,
      search: sample.search,
      fragments: Object.fromEntries(sample.fragments.map((f) => [f.name, f])),
    };
    const units = template.units.map((unit, u): Unit => {
      const other = parsed[u];
      if (
        other?.kind !== unit.kind ||
        ("start" in unit && "start" in other && unit.start !== other.start)
      ) {
        throw new Refused("Next.js's response has another structure");
      }
      const parts = match(unit.parts, other.text, values, template.learned);
      if (parts === undefined) {
        throw new Refused(
          `Next.js's response differs in ways the params and fragments don't explain (${unit.kind} part ${String(u)})`,
        );
      }
      return { ...unit, parts };
    });
    return { ok: true, template: { units, learned: template.learned } };
  } catch (error) {
    if (error instanceof Refused) return { ok: false, reason: error.message };
    throw error;
  }
}

/** Matches `parts` against `text`, deciding candidates; `undefined` if no decision matches. */
function match(
  parts: Part[],
  text: string,
  values: Values,
  learned: Record<string, string>,
): Part[] | undefined {
  const failed = new Set<string>();
  const go = (i: number, at: number): Part[] | undefined => {
    if (i === parts.length) return at === text.length ? [] : undefined;
    const key = `${String(i)}:${String(at)}`;
    if (failed.has(key)) return undefined;
    const part = parts[i];
    if (part === undefined) return undefined;
    const options: [string, Part][] = [];
    if ("candidate" in part) {
      const value = values.params[part.candidate];
      const same = value === learned[part.candidate];
      if (value !== undefined && !same)
        options.push([
          encode(value, part.encoding),
          { param: part.candidate, encoding: part.encoding },
        ]);
      options.push([part.literal, same ? part : { text: part.literal }]);
    } else {
      options.push([piece(part, values), part]);
    }
    for (const [expected, decided] of options) {
      if (!text.startsWith(expected, at)) continue;
      const rest = go(i + 1, at + expected.length);
      if (rest !== undefined) return [decided, ...rest];
    }
    failed.add(key);
    return undefined;
  };
  return go(0, 0);
}

/** The router state's URL field, as Next.js writes it: `"c"` is the path and query split on `/`. */
function url(field: "c" | "q", values: { pathname: string; search: string }): string {
  const value = field === "c" ? (values.pathname + values.search).split("/") : values.search;
  return `"${field}":${JSON.stringify(value)}`;
}

/** A decided part's text for `values`. */
function piece(part: Part, values: Values): string {
  if ("text" in part) return part.text;
  if ("fragment" in part) {
    const fragment = values.fragments[part.fragment];
    if (fragment === undefined) throw new Refused(`no fragment ${part.fragment}`);
    return part.as === "html" ? fragment.html : fragment.flight;
  }
  if ("url" in part) return url(part.url, values);
  if ("param" in part) {
    const value = values.params[part.param];
    if (value === undefined) throw new Refused(`no param ${part.param}`);
    return encode(value, part.encoding);
  }
  return part.literal;
}

/**
 * The document for `values`. A param whose candidates no response has confirmed can only take
 * the value the template was learned with; for another, the result is an error (the runtime
 * then serves the request through Next.js).
 */
export function fill(template: Template, values: Values): string {
  let out = "";
  for (const unit of template.units) {
    let text = "";
    for (const part of unit.parts) {
      if (
        "candidate" in part &&
        values.params[part.candidate] !== template.learned[part.candidate]
      ) {
        throw new Error(`param ${part.candidate} isn't confirmed for another value yet`);
      }
      text += piece(part, values);
    }
    out += unit.kind === "html" ? text : flightScript(unit.start, text);
  }
  return out;
}

/** The names of the fragments a template is filled with. */
export function fragmentNames(template: Template): Set<string> {
  const names = new Set<string>();
  for (const unit of template.units) {
    for (const part of unit.parts) if ("fragment" in part) names.add(part.fragment);
  }
  return names;
}

/** Whether every param's candidates are decided, so any values can fill the template. */
export function confirmed(template: Template): boolean {
  return template.units.every((unit) => unit.parts.every((part) => !("candidate" in part)));
}

/**
 * A page's metadata, as Next.js wrote it for one request (ADR-0022).
 *
 * Next.js 16.4 resolves a route's metadata (`metadata`, `generateMetadata`, file conventions) into
 * a list of `title`, `meta` and `link` elements. It writes them twice:
 * - in Flight, as one row, which the `Next.Metadata` boundary in the head refers to
 *   (`{"name":"Next.Metadata","children":"$L12"}`, then `12:[["$","title","0",…],…]`);
 * - in HTML, as one run of tags in `<head>`, one tag per host element, in the same order. A client
 *   element in the list (Next.js's own `IconMark`, `$L25`) writes nothing there of its own; if it
 *   did, that would be part of the wrapper, which a second response confirms (ADR-0018).
 *
 * The runtime learns both from Next.js's response for a request and treats them as a fragment of
 * that request, like the page (ADR-0018): the wrapper is captured around them.
 */
import type { Fragment } from "./capture.ts";
import { decode } from "./capture.ts";

const BOUNDARY = /\{"name":"Next\.Metadata","children":"\$L([0-9a-f]+)"\}/g;

/** Elements React DOM writes as one self-closing tag. */
const VOID = new Set(["meta", "link", "base"]);

/**
 * The metadata fragment of Next.js's `document`: `undefined` when the page has none, or a reason
 * when its metadata can't be found exactly once.
 */
export function findMetadata(document: string): Fragment | undefined | string {
  const units = decode(document);
  const flight = units.flatMap((u) => (u.kind === "flight" ? [u.text] : [])).join("");
  const boundaries = [...flight.matchAll(BOUNDARY)];
  if (boundaries.length === 0) return undefined;
  if (boundaries.length > 1) return "the metadata boundary is in Next.js's response more than once";
  const id = boundaries[0]?.[1] ?? "";
  const rows = flight.split("\n").filter((row) => row.startsWith(`${id}:`));
  if (rows.length !== 1)
    return `the metadata row is in Next.js's response ${String(rows.length)} times, not once`;
  const value = (rows[0] ?? "").slice(id.length + 1);
  let elements: unknown;
  try {
    elements = JSON.parse(value);
  } catch {
    return "the metadata row isn't a JSON value";
  }
  if (!Array.isArray(elements)) return "the metadata row isn't a list of elements";
  if (elements.length === 0) return undefined; // nothing per request: the wrapper has it
  const types: string[] = [];
  for (const element of elements) {
    if (!Array.isArray(element) || element[0] !== "$" || typeof element[1] !== "string") {
      return "the metadata row has something other than elements";
    }
    // A host element's type is its tag; a client element's is a reference (`$L25`).
    if (!element[1].startsWith("$")) types.push(element[1]);
  }
  if (types.length === 0) return undefined;
  const html = tags(units, types);
  if (html === undefined) return "the metadata's tags aren't in Next.js's <head> exactly once";
  return { name: "metadata", html, flight: value };
}

/** The one run of tags of `types`, in order, in the document's HTML; `undefined` if not one. */
function tags(units: ReturnType<typeof decode>, types: string[]): string | undefined {
  const found: string[] = [];
  for (const unit of units) {
    if (unit.kind !== "html") continue;
    const text = unit.text;
    const first = `<${types[0] ?? ""}`;
    for (let start = text.indexOf(first); start !== -1; start = text.indexOf(first, start + 1)) {
      let pos = start;
      let complete = true;
      for (const type of types) {
        const open = `<${type}`;
        const after = text[pos + open.length];
        if (!text.startsWith(open, pos) || (after !== " " && after !== ">" && after !== "/")) {
          complete = false;
          break;
        }
        // React DOM escapes `>` in attribute values, so a tag ends at the first `>`.
        const end = VOID.has(type) ? text.indexOf(">", pos) : text.indexOf(`</${type}>`, pos);
        if (end === -1) {
          complete = false;
          break;
        }
        pos = VOID.has(type) ? end + 1 : end + type.length + 3;
      }
      if (complete) found.push(text.slice(start, pos));
    }
  }
  return found.length === 1 ? found[0] : undefined;
}

/** How many requests' metadata a route keeps (the oldest learned go first). */
const REQUESTS = 10_000;

/** The metadata learned for each request (variant and URL): `null` for a page without any. */
export class MetadataCache {
  private readonly requests = new Map<string, Fragment | null>();

  get(request: string): Fragment | null | undefined {
    return this.requests.get(request);
  }

  set(request: string, metadata: Fragment | null): void {
    this.requests.delete(request);
    this.requests.set(request, metadata);
    if (this.requests.size > REQUESTS) {
      const oldest = this.requests.keys().next();
      if (oldest.done !== true) this.requests.delete(oldest.value);
    }
  }
}

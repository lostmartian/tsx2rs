/**
 * Learning a page's client component islands from Next.js's responses (ADR-0020).
 *
 * The engine can render everything around a client component, but not the component: React
 * renders it, into HTML in the page and a Flight reference to its module (`$L15`). So the runtime
 * learns both from Next.js:
 * - the first time, it renders the page with **placeholders**: a comment before and after each
 *   island's children in HTML (`<!--tsx2rs:0:b-->`, `<!--tsx2rs:0:a-->`) and a placeholder
 *   reference in Flight (`$L0`);
 * - Next.js serves that request, and in its response the runtime finds the page with each
 *   placeholder's **gap**: the island's HTML, and its reference;
 * - it **caches** each island by the request (variant and URL), its order in the page, its
 *   component and its props, and renders the same request natively from then on.
 *
 * The learned page is what Next.js wrote for that request, byte for byte, so capturing the
 * wrapper (ADR-0018) works on it as on a page without islands.
 */
import type { Island, Islands } from "./encode.ts";
import { decode } from "./capture.ts";

/** An island the page asked for while rendering, in render order. */
export interface Call {
  index: number;
  props: string;
}

/** A resolver that answers with placeholders, and the calls it was asked for. */
export function placeholders(): { islands: Islands; calls: Call[] } {
  const calls: Call[] = [];
  const islands: Islands = (index, props) => {
    const n = calls.length;
    calls.push({ index, props: key(props) });
    return {
      ref: `$L${n.toString(16)}`,
      before: `<!--tsx2rs:${String(n)}:b-->`,
      after: `<!--tsx2rs:${String(n)}:a-->`,
    };
  };
  return { islands, calls };
}

/** Props as a cache key: their JSON (what Flight writes of them). */
function key(props: Record<string, unknown>): string {
  try {
    return JSON.stringify(props);
  } catch {
    return "";
  }
}

const MARKER = /<!--tsx2rs:([0-9]+):([ab])-->/g;
const PLACEHOLDER = /"\$L([0-9a-f]+)"/g;
const REFERENCE = /^\$[A-Za-z@]?[0-9a-f]+$/;

export type LearnedIslands =
  { ok: true; islands: Island[]; html: string; flight: string } | { ok: false; reason: string };

/**
 * The islands of a page rendered with {@link placeholders} (`html`, `flight`: its fragment), from
 * Next.js's response `document` for the same request, and the page as Next.js wrote it.
 */
export function learnIslands(
  document: string,
  page: { html: string; flight: string },
  calls: Call[],
): LearnedIslands {
  const html = gapsInHtml(document, page.html, calls.length);
  if (typeof html === "string") return { ok: false, reason: html };
  const flight = refsInFlight(document, page.flight, calls.length);
  if (typeof flight === "string") return { ok: false, reason: flight };
  const islands = calls.map((_, n) => ({
    ref: flight.refs[n] ?? "",
    before: html.gaps[n]?.b ?? "",
    after: html.gaps[n]?.a ?? "",
  }));
  return { ok: true, islands, html: html.page, flight: flight.row };
}

/** Each island's HTML before and after its children, and the page's HTML in the document. */
function gapsInHtml(
  document: string,
  html: string,
  count: number,
): { gaps: { b?: string; a?: string }[]; page: string } | string {
  // Text, then (island, side, text) for each marker.
  const texts: string[] = [];
  const markers: [number, "a" | "b"][] = [];
  let at = 0;
  for (const m of html.matchAll(MARKER)) {
    texts.push(html.slice(at, m.index));
    markers.push([Number(m[1]), m[2] as "a" | "b"]);
    at = m.index + m[0].length;
  }
  texts.push(html.slice(at));
  // Every island has a `b` marker, in order; an `a` marker follows its own `b`.
  const seen = markers.filter(([, side]) => side === "b").map(([n]) => n);
  if (seen.length !== count || seen.some((n, i) => n !== i)) {
    return "the page's island markers aren't the islands it rendered";
  }
  const first = texts[0] ?? "";
  if (first === "") return "the page starts with an island";
  const found: { gaps: { b?: string; a?: string }[]; page: string }[] = [];
  for (const unit of decode(document)) {
    if (unit.kind !== "html") continue;
    const text = unit.text;
    for (let start = text.indexOf(first); start !== -1; start = text.indexOf(first, start + 1)) {
      // Each gap ends where the next text is first found.
      const gaps: { b?: string; a?: string }[] = [];
      let pos = start + first.length;
      let complete = true;
      for (const [i, [n, side]] of markers.entries()) {
        const next = texts[i + 1] ?? "";
        const end = text.indexOf(next, pos);
        if (end === -1) {
          complete = false;
          break;
        }
        gaps[n] = { ...gaps[n], [side]: text.slice(pos, end) };
        pos = end + next.length;
      }
      if (complete) found.push({ gaps, page: text.slice(start, pos) });
    }
  }
  if (found.length !== 1) {
    return `the page with its islands found ${String(found.length)} times in Next.js's response, not once`;
  }
  return found[0] ?? "";
}

/** Each island's reference, and the page's Flight row as Next.js wrote it. */
function refsInFlight(
  document: string,
  flight: string,
  count: number,
): { refs: string[]; row: string } | string {
  const pieces: string[] = [];
  const order: number[] = [];
  let at = 0;
  for (const m of flight.matchAll(PLACEHOLDER)) {
    pieces.push(flight.slice(at, m.index));
    order.push(parseInt(m[1] ?? "", 16));
    at = m.index + m[0].length;
  }
  pieces.push(flight.slice(at));
  if (order.length !== count || order.some((n, i) => n !== i)) {
    return "the page's Flight references aren't the islands it rendered";
  }
  const found: { refs: string[]; row: string }[] = [];
  for (const unit of decode(document)) {
    if (unit.kind !== "flight") continue;
    const text = unit.text;
    const first = `:${pieces[0] ?? ""}`;
    for (let start = text.indexOf(first); start !== -1; start = text.indexOf(first, start + 1)) {
      let pos = start + first.length;
      const refs: string[] = [];
      let complete = true;
      for (let i = 1; i < pieces.length && complete; i++) {
        // A reference is a JSON string: up to its closing quote.
        if (text[pos] !== '"') {
          complete = false;
          break;
        }
        const close = text.indexOf('"', pos + 1);
        const ref = close === -1 ? "" : text.slice(pos + 1, close);
        const piece = pieces[i] ?? "";
        if (!REFERENCE.test(ref) || !text.startsWith(piece, close + 1)) complete = false;
        refs.push(ref);
        pos = close + 1 + piece.length;
      }
      if (complete && text[pos] === "\n") {
        found.push({ refs, row: text.slice(start + 1, pos) });
      }
    }
  }
  if (found.length !== 1) {
    return `the page's Flight row with its islands found ${String(found.length)} times in Next.js's response, not once`;
  }
  return found[0] ?? "";
}

/** How many requests' islands a route keeps (the oldest learned go first). */
const PAGES = 10_000;

/** The islands learned for each request: by variant and URL, then by order in the page. */
export class IslandCache {
  private readonly pages = new Map<string, { calls: Call[]; islands: Island[] }>();

  /** The resolver for a request: it throws {@link Miss} for an island not learned for it. */
  resolver(request: string): Islands {
    const page = this.pages.get(request);
    let n = 0;
    return (index, props) => {
      const call = page?.calls[n];
      const island = page?.islands[n];
      n++;
      if (call?.index !== index || call.props !== key(props) || island === undefined) {
        throw new Miss();
      }
      return island;
    };
  }

  set(request: string, calls: Call[], islands: Island[]): void {
    this.pages.delete(request);
    this.pages.set(request, { calls, islands });
    if (this.pages.size > PAGES) {
      const oldest = this.pages.keys().next();
      if (oldest.done !== true) this.pages.delete(oldest.value);
    }
  }
}

/** An island the cache hasn't learned for this request. */
export class Miss extends Error {
  constructor() {
    super("an island not learned for this request");
    this.name = "Miss";
  }
}

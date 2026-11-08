/**
 * Learning what React renders inside a compiled page from Next.js's responses: client component
 * islands (ADR-0020) and the form of `<Suspense>` boundaries (ADR-0024).
 *
 * The engine renders everything around a client component, but not the component: React renders
 * it, into HTML in the page and a Flight reference to its module (`$L15`). And whether React leaves
 * a boundary pending (its fallback in the shell, its content in a segment sent after the shell) or
 * resolves it inline depends on what suspends during Next.js's render. So the runtime learns both:
 * - the first time, it renders the page with **placeholders**: comments where an island's HTML goes
 *   (`<!--tsx2rs:0:b-->`, `<!--tsx2rs:0:a-->`), every boundary in its pending form with comments
 *   for its markers (`o`, `c` around the fallback in the shell; `s`, `e` around the content in the
 *   segments), and placeholder references in Flight (`$L0`);
 * - Next.js serves that request, and in its response the runtime finds the page: each island's
 *   **gap** (its HTML), each boundary's form, read from React's own markers (`<!--$-->` resolved;
 *   `<!--$?--><template id="B:1"></template>` pending, with its segment
 *   `<div hidden id="S:1">…</div><script>…$RC("B:1","S:1")</script>`), and each reference;
 * - it **caches** them by the request (variant and URL) and their order in the page, islands also
 *   by component and props, and renders the same request natively from then on.
 *
 * The learned page is what Next.js wrote for that request, byte for byte, so capturing the
 * wrapper (ADR-0018) works on it as on any page.
 */
import type { Boundaries, Boundary, Island, Islands } from "./encode.ts";
import { decode } from "./capture.ts";

/**
 * What a page needs from what was learned: its islands and boundaries, and the ids of its row and
 * the `count` rows it defers to (row `n`'s at `n`, the page's at 0), or `undefined` to keep the
 * placeholders (`"$L!<n>"`, `"$!<n>:`).
 */
export interface Resolvers {
  islands: Islands;
  boundaries: Boundaries;
  rows: (count: number) => string[] | undefined;
}

/** What the page asked for while rendering, in render order. */
export type Call = { kind: "island"; index: number; props: string } | { kind: "boundary" };

/** What was learned for one call. */
type Part = Island | Boundary;

/** Resolvers that answer with placeholders, and the calls they were asked for. */
export function placeholders(): Resolvers & { calls: Call[] } {
  const calls: Call[] = [];
  const marker = (n: number, side: string) => `<!--tsx2rs:${String(n)}:${side}-->`;
  const islands: Islands = (index, props) => {
    const n = calls.length;
    calls.push({ kind: "island", index, props: key(props) });
    return { ref: `$L${n.toString(16)}`, before: marker(n, "b"), after: marker(n, "a") };
  };
  const boundaries: Boundaries = () => {
    const n = calls.length;
    calls.push({ kind: "boundary" });
    return {
      ref: `$L${n.toString(16)}`,
      pending: true,
      open: marker(n, "o"),
      close: marker(n, "c"),
      segment: marker(n, "s"),
      end: marker(n, "e"),
    };
  };
  return { islands, boundaries, rows: () => undefined, calls };
}

/** Props as a cache key: their JSON (what Flight writes of them). */
function key(props: Record<string, unknown>): string {
  try {
    return JSON.stringify(props);
  } catch {
    return "";
  }
}

const MARKER = /<!--tsx2rs:([0-9]+):([abocse])-->/g;
const REFERENCE = /^\$[A-Za-z@]?[0-9a-f]+$/;
const PENDING = /^<!--\$\?--><template id="B:([0-9a-z]+)"><\/template>/;
const RESOLVED = "<!--$-->";
const BOUNDARY_END = "<!--/$-->";

/** A rendered page's HTML as text and placeholder markers. */
type Token = { text: string } | { n: number; side: string };

function tokens(html: string): Token[] {
  const out: Token[] = [];
  let at = 0;
  for (const m of html.matchAll(MARKER)) {
    if (m.index > at) out.push({ text: html.slice(at, m.index) });
    out.push({ n: Number(m[1]), side: m[2] ?? "" });
    at = m.index + m[0].length;
  }
  if (at < html.length) out.push({ text: html.slice(at) });
  return out;
}

export type LearnedPage =
  | {
      ok: true;
      parts: Part[];
      /** The page as Next.js wrote it: its HTML in the shell, its segments, its Flight row, and
       * the rows it defers to (ADR-0025) with the ids Next.js gave them. */
      html: string;
      segments: string;
      flight: string;
      rows: string[];
      ids: string[];
    }
  | { ok: false; reason: string };

class Unexplained extends Error {}

/**
 * What a page rendered with {@link placeholders} (`html`, `segments`, `flight`: its fragment)
 * learns from Next.js's response `document` for the same request.
 */
export function learnPage(
  document: string,
  page: { html: string; segments: string; flight: string; rows?: string[] },
  calls: Call[],
): LearnedPage {
  try {
    const shell = tokens(page.html);
    // Each boundary's content, as rendered into the segments output.
    const contents = new Map<number, Token[]>();
    let open: { n: number; tokens: Token[] } | undefined;
    for (const token of tokens(page.segments)) {
      if ("side" in token && token.side === "s") open = { n: token.n, tokens: [] };
      else if ("side" in token && token.side === "e" && open?.n === token.n) {
        contents.set(open.n, open.tokens);
        open = undefined;
      } else open?.tokens.push(token);
    }
    const units = decode(document).flatMap((u) => (u.kind === "html" ? [u.text] : []));
    const first = shell[0];
    if (first === undefined || !("text" in first)) {
      throw new Unexplained("the page starts with an island or a boundary");
    }
    const found: { parts: Part[]; html: string; segments: string }[] = [];
    for (const text of units) {
      for (let at = text.indexOf(first.text); at !== -1; at = text.indexOf(first.text, at + 1)) {
        const walk = new Walk(text, calls, contents);
        try {
          const end = walk.tokens(shell, at);
          found.push({
            parts: walk.parts,
            html: text.slice(at, end),
            segments: walk.segments(),
          });
        } catch (error) {
          if (!(error instanceof Unexplained)) throw error;
        }
      }
    }
    if (found.length !== 1) {
      throw new Unexplained(
        `the page with its islands and boundaries found ${String(found.length)} times in Next.js's response, not once`,
      );
    }
    const learned = found[0];
    if (learned === undefined) throw new Unexplained("no page");
    const flight = matchFlight(document, [page.flight, ...(page.rows ?? [])], calls.length);
    const parts: Part[] = [];
    for (const [n, call] of calls.entries()) {
      const ref = flight.refs[n] ?? "";
      const part = learned.parts[n];
      if (part) parts.push({ ...part, ref });
      // An island in a resolved boundary's fallback: React doesn't render it, but Flight has it.
      else if (call.kind === "island") parts.push({ ref, before: "", after: "" });
      else throw new Unexplained("a boundary inside a fallback React didn't render");
    }
    return {
      ok: true,
      parts,
      html: learned.html,
      segments: learned.segments,
      flight: flight.values[0] ?? "",
      rows: flight.values.slice(1),
      ids: flight.ids,
    };
  } catch (error) {
    if (error instanceof Unexplained) return { ok: false, reason: error.message };
    throw error;
  }
}

/** One attempt to read the page in Next.js's shell, from one starting point. */
class Walk {
  readonly parts: Part[] = [];
  /** Each pending boundary's segment in the document: [start, end], by boundary. */
  private readonly found = new Map<number, [number, number, string]>();

  private readonly text: string;
  private readonly calls: Call[];
  private readonly contents: Map<number, Token[]>;

  constructor(text: string, calls: Call[], contents: Map<number, Token[]>) {
    this.text = text;
    this.calls = calls;
    this.contents = contents;
  }

  /** Reads `list` from `at`; returns where it ends. An island's gap ends where what follows it
   * is first found. */
  tokens(list: Token[], at: number, until?: string): number {
    const text = this.text;
    let pos = at;
    let gap: { n: number; side: string } | undefined;
    const close = (end: number) => {
      if (!gap) return;
      const island = (this.parts[gap.n] ?? { ref: "", before: "", after: "" }) as Island;
      this.parts[gap.n] = {
        ...island,
        [gap.side === "b" ? "before" : "after"]: text.slice(pos, end),
      };
      pos = end;
      gap = undefined;
    };
    for (let i = 0; i < list.length; i++) {
      const token = list[i];
      if (token === undefined) break;
      if ("text" in token) {
        const end = gap ? text.indexOf(token.text, pos) : pos;
        if (end === -1 || !text.startsWith(token.text, end)) throw new Unexplained("unexplained");
        close(end);
        pos = end + token.text.length;
        continue;
      }
      if (this.calls[token.n]?.kind === "island") {
        if (gap) throw new Unexplained("two islands without HTML between them");
        gap = token;
        continue;
      }
      if (token.side !== "o") throw new Unexplained("a boundary marker out of place");
      // The boundary's form, from React's own markers.
      if (gap) {
        const end = text.indexOf("<!--$", pos);
        if (end === -1) throw new Unexplained("unexplained");
        close(end);
      }
      // The fallback's tokens, up to the boundary's `c`.
      const fallback: Token[] = [];
      let j = i + 1;
      for (; j < list.length; j++) {
        const t = list[j];
        if (t && "side" in t && t.n === token.n && t.side === "c") break;
        if (t) fallback.push(t);
      }
      i = j;
      const content = this.contents.get(token.n);
      if (!content) throw new Unexplained("a boundary without its content");
      const pending = PENDING.exec(text.slice(pos, pos + 64));
      if (text.startsWith(RESOLVED, pos)) {
        pos = this.tokens(content, pos + RESOLVED.length, BOUNDARY_END);
        if (!text.startsWith(BOUNDARY_END, pos)) throw new Unexplained("unexplained");
        pos += BOUNDARY_END.length;
        this.parts[token.n] = {
          ref: "",
          pending: false,
          open: RESOLVED,
          close: "",
          segment: "",
          end: BOUNDARY_END,
        };
      } else if (pending) {
        const opened = pending[0];
        pos = this.tokens(fallback, pos + opened.length, BOUNDARY_END);
        if (!text.startsWith(BOUNDARY_END, pos)) throw new Unexplained("unexplained");
        pos += BOUNDARY_END.length;
        this.parts[token.n] = {
          ref: "",
          pending: true,
          open: opened,
          close: BOUNDARY_END,
          ...this.segment(pending[1] ?? "", token.n, content),
        };
      } else {
        throw new Unexplained("a boundary in neither of React's forms");
      }
    }
    if (gap) {
      // An island last in a fallback or a boundary's content ends where the boundary does.
      const end = until === undefined ? -1 : text.indexOf(until, pos);
      if (end === -1) throw new Unexplained("the page ends with an island");
      close(end);
    }
    return pos;
  }

  /** A pending boundary's segment: `<div hidden id="S:…">`, its content, `</div>` and the reveal
   * script naming the boundary. */
  private segment(id: string, n: number, content: Token[]): { segment: string; end: string } {
    const reveal = `$RC("B:${id}","S:`;
    const at = this.text.indexOf(reveal);
    if (at === -1 || this.text.includes(reveal, at + 1)) {
      throw new Unexplained(`boundary B:${id}'s segment isn't in Next.js's response once`);
    }
    const s = this.text.slice(at + reveal.length, this.text.indexOf('"', at + reveal.length));
    const segment = `<div hidden id="S:${s}">`;
    const start = this.text.indexOf(segment);
    if (start === -1) throw new Unexplained(`segment S:${s} isn't in Next.js's response`);
    const close = this.tokens(content, start + segment.length, "</div><script>");
    if (!this.text.startsWith("</div><script>", close)) throw new Unexplained("unexplained");
    const scriptEnd = this.text.indexOf("</script>", close);
    // The reveal call is in the script right after the segment, at its end.
    if (at < close || scriptEnd === -1 || scriptEnd < at || scriptEnd > at + 64) {
      throw new Unexplained("unexplained");
    }
    const stop = scriptEnd + "</script>".length;
    this.found.set(n, [start, stop, this.text.slice(start, stop)]);
    return { segment, end: this.text.slice(close, stop) };
  }

  /** The page's segments in the document: one run, in the page's order. */
  segments(): string {
    const spans = [...this.found.entries()].sort(([a], [b]) => a - b).map(([, span]) => span);
    if (spans.length === 0) return "";
    for (let i = 1; i < spans.length; i++) {
      // Each segment starts where the one before it ends.
      if (spans[i - 1]?.[1] !== spans[i]?.[0]) {
        throw new Unexplained("the page's segments aren't together in Next.js's response");
      }
    }
    return spans.map(([, , text]) => text).join("");
  }
}

/**
 * A placeholder in one of our rows: an island's or boundary's reference (`"$L<n>"`), a row's
 * (`"$L!<n>"`, ADR-0025), or the start of a reference to an object written before (`"$!<n>:`, its
 * row, then its path, ADR-0026).
 */
const REF = /"\$L!([0-9]+)"|"\$L([0-9a-f]+)"|"\$!([0-9]+):/g;

interface Hole {
  kind: "call" | "row" | "path";
  n: number;
}

/** One of our rows as text, and the placeholders between. */
function split(row: string): { pieces: string[]; holes: Hole[] } {
  const pieces: string[] = [];
  const holes: Hole[] = [];
  let at = 0;
  for (const m of row.matchAll(REF)) {
    pieces.push(row.slice(at, m.index));
    holes.push(
      m[1] !== undefined
        ? { kind: "row", n: Number(m[1]) }
        : m[2] !== undefined
          ? { kind: "call", n: parseInt(m[2], 16) }
          : { kind: "path", n: Number(m[3]) },
    );
    at = m.index + m[0].length;
  }
  pieces.push(row.slice(at));
  return { pieces, holes };
}

/** What a row's placeholders are in Next.js's text from `at`: where it ends, or `undefined`. */
function matchRow(
  text: string,
  at: number,
  row: { pieces: string[]; holes: Hole[] },
  found: { refs: string[]; ids: string[] },
): number | undefined {
  if (!text.startsWith(row.pieces[0] ?? "", at)) return undefined;
  let pos = at + (row.pieces[0] ?? "").length;
  // A row's id: the same wherever it's referred to.
  const id = (n: number, value: string) => {
    if (found.ids[n] !== undefined && found.ids[n] !== value) return false;
    found.ids[n] = value;
    return true;
  };
  for (const [i, hole] of row.holes.entries()) {
    if (text[pos] !== '"') return undefined;
    let end: number;
    if (hole.kind === "path") {
      // `"$<id>:`, then the path, which is the next piece.
      const m = /^"\$([0-9a-f]+):/.exec(text.slice(pos, pos + 16));
      if (!m || !id(hole.n, m[1] ?? "")) return undefined;
      end = pos + m[0].length;
    } else {
      // A reference is a JSON string: up to its closing quote.
      const close = text.indexOf('"', pos + 1);
      const ref = close === -1 ? "" : text.slice(pos + 1, close);
      if (!REFERENCE.test(ref)) return undefined;
      if (hole.kind === "row") {
        if (!ref.startsWith("$L") || !id(hole.n, ref.slice(2))) return undefined;
      } else found.refs[hole.n] = ref;
      end = close + 1;
    }
    const piece = row.pieces[i + 1] ?? "";
    if (!text.startsWith(piece, end)) return undefined;
    pos = end + piece.length;
  }
  return pos;
}

/**
 * Each placeholder's reference, each row's id, and the rows as Next.js wrote them: the page's row
 * (found once among Next.js's rows), then each row the page defers to (ADR-0025), found by the id
 * Next.js gave it.
 */
function matchFlight(
  document: string,
  rows: string[],
  count: number,
): { refs: string[]; ids: string[]; values: string[] } {
  const ours = rows.map(split);
  const calls = ours.flatMap((r) => r.holes.filter((h) => h.kind === "call").map((h) => h.n));
  if (calls.length !== count || [...calls].sort((a, b) => a - b).some((n, i) => n !== i)) {
    throw new Unexplained(
      "the page's Flight references aren't the islands and boundaries it rendered",
    );
  }
  const units = decode(document).flatMap((u) => (u.kind === "flight" ? [u.text] : []));
  const page = ours[0] ?? { pieces: [""], holes: [] };
  const candidates: { refs: string[]; ids: string[]; value: string }[] = [];
  const first = `:${page.pieces[0] ?? ""}`;
  for (const text of units) {
    for (let at = text.indexOf(first); at !== -1; at = text.indexOf(first, at + 1)) {
      // The page's row: at the start of a line, its id before the `:`.
      const line = text.lastIndexOf("\n", at) + 1;
      const own = text.slice(line, at);
      if (!/^[0-9a-f]+$/.test(own)) continue;
      const found = { refs: [] as string[], ids: [own] };
      const end = matchRow(text, at + 1, page, found);
      if (end !== undefined && text[end] === "\n") {
        candidates.push({ ...found, value: text.slice(at + 1, end) });
      }
    }
  }
  if (candidates.length !== 1 || !candidates[0]) {
    throw new Unexplained(
      `the page's Flight row with its islands found ${String(candidates.length)} times in Next.js's response, not once`,
    );
  }
  const { refs, ids, value } = candidates[0];
  const values = [value];
  for (let n = 1; n < ours.length; n++) {
    const id = ids[n];
    const row = ours[n];
    if (id === undefined || !row) throw new Unexplained(`row ${String(n)} isn't referred to`);
    // The row Next.js wrote with that id: at the start of a line.
    const starts = units.flatMap((text) => {
      const out: { text: string; at: number }[] = [];
      for (let at = text.indexOf(`${id}:`); at !== -1; at = text.indexOf(`${id}:`, at + 1)) {
        if (at === 0 || text[at - 1] === "\n") out.push({ text, at: at + id.length + 1 });
      }
      return out;
    });
    const matches = starts.flatMap(({ text, at }) => {
      const end = matchRow(text, at, row, { refs, ids });
      return end !== undefined && text[end] === "\n" ? [text.slice(at, end)] : [];
    });
    if (matches.length !== 1 || matches[0] === undefined) {
      throw new Unexplained(`row ${id} isn't in Next.js's response once as the page renders it`);
    }
    values.push(matches[0]);
  }
  return { refs, ids, values };
}

/** How many requests' learned parts a route keeps (the oldest learned go first). */
const PAGES = 10_000;

/** What was learned for each request: by variant and URL, then by order in the page. */
export class IslandCache {
  private readonly pages = new Map<string, { calls: Call[]; parts: Part[]; ids: string[] }>();

  /** The resolvers for a request: they throw {@link Miss} for anything not learned for it. */
  resolver(request: string): Resolvers {
    const page = this.pages.get(request);
    let n = 0;
    const next = () => {
      const at = n++;
      return { call: page?.calls[at], part: page?.parts[at] };
    };
    return {
      islands: (index, props) => {
        const { call, part } = next();
        if (call?.kind !== "island" || call.index !== index || call.props !== key(props) || !part) {
          throw new Miss();
        }
        return part as Island;
      },
      boundaries: () => {
        const { call, part } = next();
        if (call?.kind !== "boundary" || !part) throw new Miss();
        return part as Boundary;
      },
      rows: (count) => {
        if (page?.ids.length !== count + 1) throw new Miss();
        return page.ids;
      },
    };
  }

  set(request: string, calls: Call[], parts: Part[], ids: string[] = []): void {
    this.pages.delete(request);
    this.pages.set(request, { calls, parts, ids });
    if (this.pages.size > PAGES) {
      const oldest = this.pages.keys().next();
      if (oldest.done !== true) this.pages.delete(oldest.value);
    }
  }
}

/** `row` with each row placeholder (`"$L!<n>"`, `"$!<n>:`) given the id Next.js gave row `n`. */
export function withIds(row: string, ids: string[]): string {
  return row.replace(
    /"\$L!([0-9]+)"|"\$!([0-9]+):/g,
    (_, r: string | undefined, p: string | undefined) =>
      r === undefined ? `"$${ids[Number(p)] ?? ""}:` : `"$L${ids[Number(r)] ?? ""}"`,
  );
}

/** Something the cache hasn't learned for this request. */
export class Miss extends Error {
  constructor() {
    super("an island or boundary not learned for this request");
    this.name = "Miss";
  }
}

/**
 * A minimal tokenizer for server-rendered HTML: just enough to match elements and React's
 * comment markers. It understands quoted attribute values, comments, and raw-text elements, so a
 * `<div` inside a `<script>` or a JSON-LD block can't be mistaken for markup.
 */
import { IdentityError } from "./errors.ts";

/** Elements whose content is raw text: markup inside them is not parsed. */
const RAW_TEXT_ELEMENTS = new Set(["script", "style", "textarea", "title", "noscript"]);

const TOKEN = /<!--([\s\S]*?)-->|<(\/?)([a-zA-Z][\w:-]*)((?:"[^"]*"|'[^']*'|[^'">])*)>/g;

export type Token =
  | {
      readonly kind: "comment";
      readonly start: number;
      readonly end: number;
      readonly data: string;
    }
  | {
      readonly kind: "open" | "close";
      readonly start: number;
      readonly end: number;
      readonly tag: string;
      readonly attributes: string;
    };

/** Tokens from `from` onwards, in document order. Raw-text element content is skipped. */
export function* tokens(html: string, from = 0): Generator<Token> {
  const re = new RegExp(TOKEN.source, "g");
  re.lastIndex = from;
  for (let m = re.exec(html); m !== null; m = re.exec(html)) {
    const [whole, comment, slash, rawTag, attributes] = m;
    const start = m.index;
    const end = start + whole.length;
    if (comment !== undefined) {
      yield { kind: "comment", start, end, data: comment };
      continue;
    }
    const tag = (rawTag ?? "").toLowerCase();
    const kind = slash === "/" ? "close" : "open";
    yield { kind, start, end, tag, attributes: attributes ?? "" };
    if (kind === "open" && RAW_TEXT_ELEMENTS.has(tag)) {
      const close = html.toLowerCase().indexOf(`</${tag}`, end);
      re.lastIndex = close === -1 ? html.length : close;
    }
  }
}

/** The value of attribute `name` in an open tag's attribute text, or undefined if absent. */
export function attribute(attributes: string, name: string): string | undefined {
  const re = new RegExp(`(?:^|\\s)${name}(?:\\s*=\\s*(?:"([^"]*)"|'([^']*)'|([^\\s"'=<>\`]+)))?`);
  const m = re.exec(attributes);
  if (m === null) return undefined;
  return m[1] ?? m[2] ?? m[3] ?? "";
}

/** The element whose open tag has `id="<id>"`: its span and its inner HTML span. */
export interface ElementSpan {
  readonly tag: string;
  readonly attributes: string;
  readonly start: number;
  readonly end: number;
  readonly innerStart: number;
  readonly innerEnd: number;
}

/**
 * Finds the element with the given id (balanced over elements of the same tag name).
 * Returns undefined if no element has the id; throws if the element is never closed.
 */
export function findElementById(html: string, id: string): ElementSpan | undefined {
  for (const open of tokens(html)) {
    if (open.kind !== "open" || attribute(open.attributes, "id") !== id) continue;
    let depth = 0;
    for (const t of tokens(html, open.start)) {
      if (t.kind === "comment" || t.tag !== open.tag) continue;
      if (t.kind === "open" && !t.attributes.trimEnd().endsWith("/")) depth++;
      if (t.kind === "close" && --depth === 0) {
        return {
          tag: open.tag,
          attributes: open.attributes,
          start: open.start,
          end: t.end,
          innerStart: open.end,
          innerEnd: t.start,
        };
      }
    }
    throw new IdentityError("MALFORMED_STREAM", `<${open.tag} id="${id}"> is never closed`);
  }
  return undefined;
}

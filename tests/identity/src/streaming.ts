/**
 * Applies React's streaming instructions to a server-rendered document, producing the HTML the
 * browser ends up with once every streamed `<Suspense>` boundary has arrived.
 *
 * Next.js decides per request which boundaries stream, depending on timing, so two correct
 * responses can differ byte for byte before resolution and agree after it (ADR-0002).
 *
 * Semantics follow React DOM 19.3.1 (`react-dom-server.node.production.js`):
 * - `$RC(boundaryId, segmentId)` (completeBoundary + `$RV`): the comment before
 *   `<template id=boundaryId>` becomes `<!--$-->`; the template and the fallback up to the
 *   matching `<!--/$-->` are removed; the hidden segment's children take their place; the hidden
 *   segment is removed. If the template is gone, only the segment is removed.
 * - `$RS(segmentId, placeholderId)` (completeSegment): the hidden segment's children replace
 *   `<template id=placeholderId>`; the segment is removed.
 * - `$RX` (client-rendered error boundary) and `$RR` (boundary with stylesheets) are refused.
 */
import { IdentityError } from "./errors.ts";
import { attribute, findElementById, tokens } from "./html.ts";

export interface StreamInstruction {
  readonly kind: "completeBoundary" | "completeSegment";
  readonly args: readonly [string, string];
}

export interface ResolvedDocument {
  /** The document after all instructions were applied, without the instruction scripts. */
  readonly html: string;
  /** The instructions that were applied, in document order. */
  readonly applied: readonly StreamInstruction[];
}

/** Comment markers that open and close a boundary (React DOM's `$RV` walk). */
const BOUNDARY_OPEN = new Set(["$", "$?", "$~", "$!", "&"]);
const BOUNDARY_CLOSE = new Set(["/$", "/&"]);

const INSTRUCTION_CALL = /\$(RC|RS|RX|RR)\(([^()]*)\)\s*;?\s*$/;
const ANY_INSTRUCTION = /\$R[CSXR]\(/;

interface InstructionScript {
  readonly start: number;
  readonly end: number;
  readonly name: string;
  readonly args: readonly string[];
}

export function resolveStreamedHtml(html: string): ResolvedDocument {
  if (/<template\s+data-rsi/.test(html)) {
    throw new IdentityError(
      "UNSUPPORTED_STREAM_INSTRUCTION",
      "instructions delivered as <template data-rsi> (external runtime) are not modelled",
    );
  }
  const applied: StreamInstruction[] = [];
  let doc = html;
  for (let script = nextInstruction(doc); script !== undefined; script = nextInstruction(doc)) {
    const [first, second] = script.args;
    if (script.args.length !== 2 || first === undefined || second === undefined) {
      throw new IdentityError(
        script.name === "RX" || script.name === "RR"
          ? "UNSUPPORTED_STREAM_INSTRUCTION"
          : "MALFORMED_STREAM",
        `$${script.name}(${script.args.join(",")}) is not a two-id instruction`,
      );
    }
    switch (script.name) {
      case "RC":
        doc = completeBoundary(removeRange(doc, script.start, script.end), first, second);
        applied.push({ kind: "completeBoundary", args: [first, second] });
        break;
      case "RS":
        doc = completeSegment(removeRange(doc, script.start, script.end), first, second);
        applied.push({ kind: "completeSegment", args: [first, second] });
        break;
      default:
        throw new IdentityError(
          "UNSUPPORTED_STREAM_INSTRUCTION",
          `$${script.name} (${script.name === "RX" ? "client-rendered error boundary" : "boundary with stylesheets"}) is not modelled`,
        );
    }
  }
  return { html: doc, applied };
}

function nextInstruction(html: string): InstructionScript | undefined {
  for (const t of tokens(html)) {
    if (t.kind !== "open" || t.tag !== "script") continue;
    const close = html.toLowerCase().indexOf("</script>", t.end);
    if (close === -1) throw new IdentityError("MALFORMED_STREAM", "unterminated <script>");
    const body = html.slice(t.end, close);
    if (!ANY_INSTRUCTION.test(body)) continue;
    const call = INSTRUCTION_CALL.exec(body);
    if (call === null) {
      throw new IdentityError(
        "MALFORMED_STREAM",
        `script with a streaming instruction that doesn't end in one call: ${body.slice(-80)}`,
      );
    }
    const [, name = "", rawArgs = ""] = call;
    return { start: t.start, end: close + "</script>".length, name, args: parseArgs(rawArgs) };
  }
  return undefined;
}

/** Instruction arguments are JS literals that are also valid JSON (`"B:0","S:0"`, `null`). */
function parseArgs(raw: string): string[] {
  let parsed: unknown;
  try {
    parsed = JSON.parse(`[${raw}]`);
  } catch {
    throw new IdentityError("MALFORMED_STREAM", `unparseable instruction arguments: ${raw}`);
  }
  return (parsed as unknown[]).map((a) => (typeof a === "string" ? a : JSON.stringify(a)));
}

/** The hidden container React streams a segment's content in. Only `<div hidden>` is modelled. */
function hiddenSegment(html: string, id: string) {
  const segment = findElementById(html, id);
  if (segment === undefined) return undefined;
  if (segment.tag !== "div" || attribute(segment.attributes, "hidden") === undefined) {
    throw new IdentityError(
      "UNSUPPORTED_STREAM_INSTRUCTION",
      `segment ${id} is streamed in <${segment.tag}> (only <div hidden> is modelled)`,
    );
  }
  return segment;
}

function completeBoundary(html: string, boundaryId: string, segmentId: string): string {
  const segment = hiddenSegment(html, segmentId);
  if (segment === undefined) return html; // React: `if (b = getElementById(b))` — nothing to do
  const content = html.slice(segment.innerStart, segment.innerEnd);
  const withoutSegment = removeRange(html, segment.start, segment.end);

  const template = findElementById(withoutSegment, boundaryId);
  if (template === undefined) return withoutSegment; // React removes only the segment
  if (template.tag !== "template") {
    throw new IdentityError(
      "MALFORMED_STREAM",
      `${boundaryId} is a <${template.tag}>, not a <template>`,
    );
  }
  const pending = "<!--$?-->";
  const commentStart = template.start - pending.length;
  if (withoutSegment.slice(commentStart, template.start) !== pending) {
    throw new IdentityError(
      "MALFORMED_STREAM",
      `boundary ${boundaryId} is not directly preceded by ${pending}`,
    );
  }
  const close = matchingBoundaryClose(withoutSegment, template.start);
  return withoutSegment.slice(0, commentStart) + "<!--$-->" + content + withoutSegment.slice(close);
}

function completeSegment(html: string, segmentId: string, placeholderId: string): string {
  const segment = hiddenSegment(html, segmentId);
  if (segment === undefined) {
    throw new IdentityError("MALFORMED_STREAM", `$RS: segment ${segmentId} not found`);
  }
  const content = html.slice(segment.innerStart, segment.innerEnd);
  const withoutSegment = removeRange(html, segment.start, segment.end);
  const placeholder = findElementById(withoutSegment, placeholderId);
  if (placeholder?.tag !== "template") {
    throw new IdentityError("MALFORMED_STREAM", `$RS: placeholder ${placeholderId} not found`);
  }
  return (
    withoutSegment.slice(0, placeholder.start) + content + withoutSegment.slice(placeholder.end)
  );
}

/**
 * Start index of the comment that closes the boundary containing the node at `from`
 * (the walk in React DOM's `$RV`: nested boundaries increase the depth, closers decrease it).
 */
function matchingBoundaryClose(html: string, from: number): number {
  let depth = 0;
  for (const t of tokens(html, from)) {
    if (t.kind !== "comment") continue;
    if (BOUNDARY_CLOSE.has(t.data)) {
      if (depth === 0) return t.start;
      depth--;
    } else if (BOUNDARY_OPEN.has(t.data)) {
      depth++;
    }
  }
  throw new IdentityError("MALFORMED_STREAM", `no closing <!--/$--> after index ${String(from)}`);
}

function removeRange(html: string, start: number, end: number): string {
  return html.slice(0, start) + html.slice(end);
}

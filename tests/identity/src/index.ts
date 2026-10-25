/**
 * The identity harness: decides whether a tsx2rs response is identical to Next.js's (ADR-0002).
 */
export { IdentityError, type IdentityErrorCode } from "./errors.ts";
export {
  decodeFlight,
  extractFlight,
  parseRows,
  type Decoded,
  type DecodedFlight,
  type ExtractedFlight,
  type Row,
  type Rows,
  type Tagged,
} from "./flight.ts";
export { resolveStreamedHtml, type ResolvedDocument, type StreamInstruction } from "./streaming.ts";

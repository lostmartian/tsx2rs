/**
 * The identity harness: decides whether a tsx2rs response is identical to Next.js's (ADR-0002).
 */
export {
  compareDocuments,
  firstHtmlDifference,
  firstTreeDifference,
  normalizeDocument,
  type FlightDifference,
  type HtmlDifference,
  type IdentityReport,
  type NormalizedDocument,
} from "./compare.ts";
export { IdentityError, type IdentityErrorCode } from "./errors.ts";
export { listFixtures, loadFixture, parseExpectations, type Fixture } from "./fixtures.ts";
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
export { buildNextApp, startNextApp, type RunningServer } from "./next.ts";
export { resolveStreamedHtml, type ResolvedDocument, type StreamInstruction } from "./streaming.ts";

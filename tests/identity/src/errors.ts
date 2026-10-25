/** Stable codes for every way the harness can refuse an input. */
export type IdentityErrorCode =
  /** A React streaming instruction the harness doesn't model (e.g. `$RX`, `$RR`). */
  | "UNSUPPORTED_STREAM_INSTRUCTION"
  /** Streamed HTML that doesn't have the shape React produces. */
  | "MALFORMED_STREAM"
  /** A Flight row type the harness doesn't model (e.g. binary typed-array rows). */
  | "UNSUPPORTED_FLIGHT_ROW"
  /** A Flight `$` value the harness doesn't model (e.g. temporary references). */
  | "UNSUPPORTED_FLIGHT_VALUE"
  /** A Flight payload that doesn't follow React's row grammar. */
  | "MALFORMED_FLIGHT"
  /** A Flight reference to a row that doesn't exist. */
  | "DANGLING_FLIGHT_REFERENCE"
  /** A Flight reference that refers back to itself. */
  | "CYCLIC_FLIGHT_REFERENCE";

/**
 * The harness never guesses: input it can't interpret exactly throws this error instead of
 * producing a comparison that might hide a difference.
 */
export class IdentityError extends Error {
  readonly code: IdentityErrorCode;

  constructor(code: IdentityErrorCode, message: string) {
    super(`${code}: ${message}`);
    this.name = "IdentityError";
    this.code = code;
  }
}

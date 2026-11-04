# ADR-0016: The generated encoder: straight-line code per template, on a small runtime library

- **Status:** Accepted
- **Date:** 2026-11-04

## Context

Step 6 of milestone 1 is the encoder. It writes a data function's record as the engine's input
(D-016, ADR-0005 section 5).
- ADR-0012 says it's generated from the same traversal as the program, so their orders can't
  diverge.
- ADR-0015 added a reference resolver and encoder to the component fixtures. It interprets a
  record's shape at runtime. The generated encoder must write the same bytes.

The encoder also resolves pending calls and key paths as it walks (ADR-0013, ADR-0008). Its
walk is in program order, so it reaches each pending call after the caller's record is complete,
in React's order.

## Decision

- **A runtime library, `@tsx2rs/runtime/encode`:**
  - **Writer:** `Input` writes the engine's format: tags, `u32` counts, `f64`, UTF-8 strings.
  - **Value encoders,** one per hole kind (ADR-0005, section 5):
    - `node`, for `Op::Child`: text, numbers, bigints, booleans, `null`, `undefined`, arrays, and
      host elements as element nodes
    - `primitive`, for `Op::Prop`
    - `style`
    - `attrs`, for `Op::Attributes`
    - `value`, for `Op::Value`: primitives, arrays and plain objects
  - **Resolution:** `call` resolves a pending call as `jsx` does and runs the callee's data
    function with the callee's key path. `rootKey` puts the key path in front of a root element's
    key.
  - **Unsupported values:** a value the engine can't take natively is a `NotNative` error
    carrying the reason and the value. This is where the hand-off to React (ADR-0005,
    section 10) goes in step 8. Examples:
    - a promise
    - a function
    - a component element
    - a non-array iterable
    - a `Date`
    - an object as a child or attribute
- **The generated code (`compiler::encoder`)** is one JavaScript function per template of a
  route's program, from its `Slot` tree:
  - Each slot is a line or a small block: index the record, call the encoder for its kind,
    branch, loop. Nothing is interpreted at runtime.
  - Called templates (`Op::Call`) are functions of their own. An inlined callee is an arrow
    function passed to `call`.
  - The callees' data functions come in as an array, in the order of `Encoder::callees` (module,
    component). Names alone would collide across modules, and how modules are bundled is step
    7's.
  - The module exports `encode(record, data)`, which returns the input bytes.
- **Checked by the fixtures:** each case is encoded by the generated encoder *and* the reference.
  The bytes must be equal, and the engine's render of them must match React. Two independent
  implementations of one contract.

## Options considered

- **Interpret the plan at runtime,** as the reference does. Simpler, but it walks a data
  structure per value, on the request path. Rejected; it stays as the test's second
  implementation.
- **Encode in Rust (napi)** from JavaScript values. Crossing into Rust per value costs more than
  writing bytes in JavaScript, and D-016 measured the JavaScript encoder with a single crossing.
  Rejected.
- **Import data functions by name from their modules.** It couples the encoder to a module layout
  that step 7 decides. Rejected for an array the adapter fills.

## Consequences

- `@tsx2rs/runtime` exists, with `encode` as its first module. The HTTP server comes in step 8.
- `tests/differential` depends on it, and the fixtures run the product's encoder.
- Template nodes, for component elements found at runtime, come with the runtime's component
  table (step 8): until then they are `NotNative`.

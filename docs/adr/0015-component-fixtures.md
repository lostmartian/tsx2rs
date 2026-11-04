# ADR-0015: Component fixtures run the whole compiler pipeline, and what they changed in records

- **Status:** Accepted
- **Date:** 2026-11-04

## Context

ADR-0012 (section 6) made fixtures step 5's acceptance test. A small `.tsx` component is compiled,
its data function run in Node.js, its record encoded, and the program rendered by the engine.
The result is compared with React rendering the original component.

Two pieces don't exist yet: the runtime that resolves pending calls and key paths (step 8), and
the generated encoder (step 6). The fixtures need both.

Once the fixtures ran, they found four places where records didn't say enough, or said it in the
wrong order.

## Decision

- **The harness** (`tests/differential`, `src/components.rs` and `oracle/components.ts`):
  - **Fixtures:** each `components/*.tsx` is a page (its default export) plus
    `export const cases`, one props object per case.
  - **Compile:** Rust compiles the page with `lower::linked` and generates every component's data
    function. It sends Node.js the data module and a *plan*: the record's shape, as JSON.
  - **Run:** Node.js compiles both modules with **the SWC Next.js uses**
    (`next/dist/build/swc`), so the JSX transform's evaluation order is the real one. It runs the
    data function, then resolves and encodes the record. This is a *reference* of the runtime and
    encoder, written from the record contract, which the generated encoder (step 6) must match.
  - **Compare:** Rust renders the program. React renders the original page with React DOM's
    server and React's Flight server (production), and the HTML and Flight payload must be
    identical.
  - **Coverage:** the fixture test runs with the differential tests, in the same CI job.
  - **Mutation check:** planted compiler bugs must fail it (`tools/mutations/fixtures.json` in the
    lab repository).
- **Records, as changed by what the fixtures found:**
  - **An element whose key is known only at runtime is one group,
    `Slot::Keyed { record, .. }`.** That covers a component's root element, a dynamic key, and a
    spread, which may hold a `key`. Its value is `[key, ...record]`. It replaces `Slot::RootKey`
    and `ValueKind::Key`.
    - The key is evaluated where SWC evaluates it. Without a spread, `jsx(type, config, key)`
      evaluates it after the props and children (`__tsx2rs_keyed`). With a spread,
      `__tsx2rs_el(config, record, key)` resolves it as `jsx` does: `config.key` wins, and the
      props leave `key` out. When the key comes after a spread (`createElement`), it's written
      inside `config` in its place.
    - For a root element, the runtime puts the key path in front of that key (ADR-0008).
  - **An element a component returns through a conditional is its root element too,** with the
    key path and implicit slot. React wraps `return a ? <b key="k"/> : …` as
    `[["$","b","k",…]]` when the component has no key.
  - **A component's record is always an array of slot values.** With several returns it is
    `[[index, record]]`: one `Returns` slot. A list callback's single return is just its record.
  - **A list item that reads nothing** reads its always-true condition without a value of its
    own: the item's value is its record.
- **Out of the fixtures, for now:**
  - pages that hoist into `<head>` (`<img>`, `<link>`, `<title>`), which the engine's own
    differential tests cover as whole documents
  - multi-module fixtures, which need the bundler's module resolution (step 7)
  - async components and Suspense (step 8)

## Options considered

- **Encode in Rust from a JSON record.** Pending calls have to call JavaScript data functions,
  so the resolver must run in Node.js anyway. One reference implementation, in JavaScript, is
  closer to what ships.
- **Transform JSX with TypeScript or esbuild.** These agree with SWC on the cases tested so far,
  but Next.js uses SWC, and evaluation order is what's being tested. Rejected.
- **Keep flat records, and evaluate keys first.** It's simpler, but different from React
  whenever a key expression has side effects, and wrong when a spread holds a `key`. Rejected.

## Consequences

- The record contract is now checked end to end. The generated encoder (step 6) gets a
  reference to match and a fixture suite to run against.
- `tests/differential` depends on `next` (for its SWC) and on `tsx2rs_compiler`.

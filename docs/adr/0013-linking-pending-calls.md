# ADR-0013: Linking by inlining, with pending calls as `jsx` arguments

- **Status:** Accepted
- **Date:** 2026-11-04

## Context

ADR-0005 (sections 4 and 6) and ADR-0012 (section 5) decided that a compiled child component is
inlined into its caller's program by default. Its data comes from its own data function, called
with a *pending call* once the caller's record is complete, as React runs a child after its
parent. Two things were left open:
- what a pending call looks like in the record;
- what the first linking pull request covers.

Next.js compiles JSX with SWC's automatic runtime, which writes a component element in one of two
forms. Probed with the SWC in Next.js 16.4:
- `jsx(C, { …props, children }, key)` when the key comes before any spread, or there's no
  spread. The key is evaluated **after** the props.
- `createElement(C, { …, key: k(), …, children })` when the key comes after a spread. The key is
  evaluated **in its place** among the props.

In both, React 19 takes a `key` inside the props object over the argument. It's left out of the
props the component receives.

## Decision

- **A pending call is `[config, key]`:** the two arguments of `jsx`, built and evaluated as SWC
  builds them.
  - A key after a spread is written inside `config` in its place, and the argument is
    `undefined`.
  - Children are the last entry of `config`.
  - The runtime (step 8) resolves the call as `jsx` does:
    - the key is `config.key` if it isn't `undefined`, otherwise the argument; either becomes a
      string
    - the props are `config` without `key`
  - It then calls the callee's data function with those props after the caller's record is
    complete. It does this depth first and in program order, which is the order React's Flight
    server visits children.
  - The record holds no reference to the callee. The encoder knows it statically from
    `Slot::Call` (module and component index), so the data function needs no extra imports.
- **The callee's template is inlined.** It is lowered inside the caller's program, at the
  element's place, so chunks merge across the boundary and text separators come out as React's.
  Its own spans are in its own module's source, and its own children resolve from its own
  module.
- **What links, in the first pull request (C5):** a component element links when:
  - its tag is a module-level binding (checked with `oxc_semantic`, so a parameter or a local
    with the same name doesn't link)
  - the module graph resolves the tag to a server component, in the same module or through
    imports and re-exports
  - the callee is synchronous, calls no hook, and isn't already being inlined (no recursion)
  - its children are only text, literals and expressions, and it has no JSX props
  - the callee lowers. If it can't, the partial inlining is undone.

  Anything else stays a `Child` hole, a value the runtime renders, which is correct but
  slower.
- **What follows in a second pull request (C5b):**
  - slots: JSX children and JSX props, placed where the callee places them
  - `Call` and the template table, for recursion and above a size limit
  - template nodes, for components known only at runtime
  - async callees, with the encoder's awaiting

## Options considered

- **The pending call as a plain props object, with the key separate and evaluated first.**
  Simpler to read, but it changes evaluation order whenever a key expression has side effects,
  and gets the key wrong when a spread holds one. Rejected.
- **The data function calls the callee's data function in place.** This would run the child
  before the parent's later expressions, which isn't React's order. Rejected (ADR-0005,
  section 4).
- **The record names the callee's data function.** Every caller would need an import of it, and
  the shipped module layout isn't settled. The encoder knows the callee statically anyway.
  Rejected.
- **Link everything at once, slots and `Call` included.** It's one large change, and the most
  common case (Commerce's `Price`, `Label`, `Prose`) needs none of it. Split.

## Consequences

- `Slot::Call` is part of the record's contract with the encoder (step 6) and the runtime
  (step 8).
- Host elements have the same question of key order: their key is read before their props.
  That case is tracked for the fixtures (C6).
- Recursion, slots and async children still render correctly, through the runtime. Only speed
  is lost until C5b.

# ADR-0029: Data functions memoize `Intl` formatters

- **Status:** Accepted
- **Date:** 2026-11-08

## Context

Commerce's `Price` builds `new Intl.NumberFormat(undefined, { style: "currency", … })` on every
render, about 20 times per product page. Profiling tsx2rs serving the page: 18% of its CPU was
there. Next.js pays the same, but Phase 0's hand-written server memoized formatters (its note: they
are immutable, so memoizing them by locale and options gives the same output; ~40% of its CPU
otherwise), and milestone 1 measures tsx2rs against that server.

## Decision

- **The compiler finds `new Intl.X(…)`** in each component, for the global `Intl` (no binding in
  scope) and the formatters whose instances only format (`Collator`, `DateTimeFormat`,
  `DisplayNames`, `ListFormat`, `NumberFormat`, `PluralRules`, `RelativeTimeFormat`), without a
  spread argument.
- **Its data function calls `__tsx2rs_intl(Intl.X, …args)` instead**, in its body and in the
  template holes it copies. The helper keeps one instance per formatter and arguments (as JSON) when
  the arguments are plain data (strings, numbers, booleans, `null`, `undefined`, arrays and plain
  objects of those), up to 1,000; anything else gets `new Intl.X(…)` as written.
- Only tsx2rs's data functions change. Next.js's own render of the component is the app's code as
  written.

## Consequences

- The same output: a formatter's result depends only on its arguments and the value formatted.
  Code that compares formatter instances by identity, or sets properties on one, would see a
  shared instance; that would be a strange use of a formatter.
- On Commerce's product page, `Price` went from 18% of the CPU to under 1%.

# ADR-0002: Output identical to Next.js, or fall back

- **Status:** Accepted
- **Date:** 2026-10-24

## Context

tsx2rs replaces Next.js's rendering pipeline for some routes. Users must not be able to tell the difference except in speed: hydration, client navigation, caching and SEO all depend on the exact output. Phase 0 research showed this is achievable for real apps (Vercel Commerce: 9/9 pages identical). It also showed that Next.js's streaming chunk boundaries vary with timing.

## Decision

For a compiled route:
- The **HTML** is byte-identical to `next start`, after applying streamed `<Suspense>` boundaries the way the browser does and removing Flight script tags.
- The **Flight** payload decodes to a deep-equal tree, with all references resolved.

When the compiler cannot prove it will produce identical output, the route **falls back** to Next.js's own handler with a stated reason. When something fails at request time, that request falls back too. If headers were already sent, it errors instead; it never produces partial wrong output.

## Options considered

- **"Equivalent DOM" instead of identical bytes:** easier, but hides hydration mismatches and SEO-relevant differences. Rejected.
- **Byte-identical streams:** impossible, because Next.js's own chunking is timing-dependent. Rejected in favour of comparing after streams are resolved.

## Consequences

- The identity harness (`tests/identity`) is the main correctness test, and every supported construct has a fixture.
- Known representational differences that decode to equal data (e.g. React's by-reference object dedupe in Flight) are documented as fidelity gaps and tracked until closed.

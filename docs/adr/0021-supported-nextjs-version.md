# ADR-0021: One supported Next.js version, moved deliberately

- **Status:** Accepted
- **Date:** 2026-11-08

## Context

tsx2rs reproduces what Next.js writes, byte for byte: its wrapper (ADR-0018), its routing
(ADR-0019) and its escaping (the engine mirrors `htmlescape.js`). All of these are internal details
of one Next.js release. ADR-0019 made the runtime hand every request to Next.js when the installed
or building version isn't the supported one, 16.4.3.

Next.js 16.4.5 came out in Dependabot's weekly npm group, together with tooling updates. The group
failed CI, correctly: the adapter's test asserts the supported version.

## Decision

- **tsx2rs supports exactly one Next.js version at a time**, `NEXT_VERSION` in
  `@tsx2rs/runtime`. Any other version is served entirely by Next.js, as ADR-0019 says.
- **Moving it is its own PR**, which:
  - moves every pin together: the workspace catalog, `@next/routing`, the packages' peer
    dependencies, the test apps, `NEXT_VERSION`;
  - diffs the Next.js internals tsx2rs mirrors (`htmlescape.js`) and updates the engine if they
    changed;
  - refreshes the saved Next.js documents the unit tests use (`packages/runtime/test/next`);
  - passes the identity suites against the new version: the Next.js fixtures, the adapter's
    builds, and the runtime's end-to-end tests, Commerce included, byte for byte.
- **Dependabot proposes Next.js on its own**, outside the minor group, so a Next.js release never
  holds back tooling updates.
- **The supported version is now 16.4.5.** Its `htmlescape.js` is unchanged from 16.4.3.

## Options considered

- **A range of versions (`16.4.x`).** Patch releases can change the wrapper's bytes. Without
  running the identity suites on each, tsx2rs could serve a page that differs from what Next.js
  would write, which breaks identical-or-fallback (ADR-0002).
- **Stay on 16.4.3.** Users upgrading Next.js would silently lose tsx2rs's speed.

## Consequences

- Each Next.js release needs a tsx2rs release before apps on it are served natively. That's the
  cost of being byte-identical. Automating the move PR is possible later.
- ADR-0019's "16.4.3" is the version at that time; this ADR says how it moves.

# ADR-0027: Served routes are re-checked against Next.js

- **Status:** Accepted
- **Date:** 2026-11-08

## Context

A served route fills what it learned from Next.js's responses: the wrapper (ADR-0018, ADR-0023),
islands and boundaries (ADR-0020, ADR-0024), metadata (ADR-0022), row ids (ADR-0025). Each was
right when it was learned. Data behind it can change later (a CMS menu gets an item, a product's
title changes), and tsx2rs would keep sending the old bytes: identical-or-fallback (ADR-0002) has to
hold over time, not only at learning.

## Decision

- **Every `verifyEvery`th request a route would serve, Next.js answers instead** (default 100,
  `verifyEvery` in the server's options, `--verify-every` on the command line; 0 turns it off).
  The route compares the document it would have sent with Next.js's, byte for byte.
- **A difference forgets what was learned for that request:** its islands, boundaries, row ids and
  metadata, and its own wrapper; or, for a shared wrapper, the route relearns it from the next
  responses (learn, then confirm).
- **The route counts its re-checks** (`compared`, `differed`), shown on the status page.

## Consequences

- Stale output is caught within `verifyEvery` requests of the route, and corrected after the next
  Next.js responses. Between a change and its re-check, a route can still send the old bytes; the
  rate trades that window against the cost of Next.js rendering 1% of requests.
- The comparison doubles as a running identity test in production.

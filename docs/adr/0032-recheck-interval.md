# ADR-0032: Re-checks are limited in time as well as by count

- **Status:** Accepted
- **Date:** 2026-11-09
- **Supersedes:** [ADR-0027](0027-re-checking.md)

## Context

ADR-0027 re-checks a served route every `verifyEvery`th request (100 by default): Next.js answers
that request, and the route compares the document it would have sent with Next.js's.

On Google Cloud after the speed work (lab D-059), re-checks cost tsx2rs **12–13% of its throughput
at 1 process and 17–22% at 8** on Commerce's product pages. Measured on the laptop, a re-check's
cost is one Next.js render: Next.js's handler took the whole re-check (31.1 of 31.1 ms), and
capturing and comparing the response took about 0.02 ms. A cheaper comparison would save nothing.

A count alone makes the cost grow with traffic. At 4,000 requests a second, every 100th request is
40 Next.js renders a second for one route: far more checking than needed to notice stale output, at
a cost that grows as tsx2rs gets faster (a Next.js render stays the same price).

## Decision

- **A route re-checks once both are true:**
  - at least `verifyEvery` requests it would serve have passed since its last re-check (default
    100, as before);
  - at least **`verifyInterval` ms** have passed since its last re-check (**default 1,000**,
    `verifyInterval` in the server's options, `--verify-interval` on the command line; 0 turns the
    limit off and gives ADR-0027's behaviour).
- **It is per route and per process**, as the count was. It isn't per URL: per URL, every URL's
  first served request would be a re-check, which on a large catalogue with rarely seen URLs would
  send much of the traffic to Next.js.
- Everything else from ADR-0027 stays:
  - a difference forgets what was learned for that request (or relearns a shared wrapper);
  - the route counts `compared` and `differed`;
  - `verifyEvery` 0 turns re-checks off.

## Consequences

- **The cost is bounded:**
  - at most 1% of a route's requests (the count);
  - at most one Next.js render a second per route per process (the time), about 0.5% of a core for
    this page.
  - Busy routes recover most of the 12–22%. Quiet routes (under 100 requests a second) behave as
    before.
- **Stale output is still caught soon on any route with traffic:** within a second, or within 100
  requests, whichever is later. A stale URL that gets a small share of a busy route's traffic is
  sampled less often than before. Each re-check picks whichever request arrives, as the count did.
- **Each worker of `tsx2rs start --workers n` re-checks on its own:** up to `n` re-checks a second
  per route.

## Options considered

- **A cheaper comparison** (bytes instead of decoded text): measured to save nothing, since the
  render is the cost.
- **Re-checking a URL less often after many clean checks:** needs per-URL state, and still grows
  with traffic on busy URLs.
- **Rendering the re-check elsewhere** (a worker thread): the same CPU, and at full load every core
  is already busy.

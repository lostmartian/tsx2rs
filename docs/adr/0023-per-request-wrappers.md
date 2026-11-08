# ADR-0023: A wrapper that depends on the request is kept per URL

- **Status:** Accepted
- **Date:** 2026-11-08

## Context

A route's wrapper (ADR-0018) is learned from one response and confirmed with another for other
params. Params, the URL in the router state, the page and its metadata (ADR-0022) are slots; any
other difference refused the route.

On Commerce, the CMS pages (`/[page]`) differ in the layout: its footer menu marks the current
page's link (a client component reading the pathname, rendered inside the layout, which tsx2rs
doesn't compile). The wrapper depends on the URL itself, so no shared template can explain it.

## Decision

- **When confirming fails, each URL keeps the wrapper learned from its own response.** A variant
  switches to per-request wrappers: the first two URLs' templates are kept, and each new URL's
  first response teaches it.
- **Replaying needs repeatable answers.** Before any per-request wrapper is served, one URL's second
  response must match its first (with its own fragments). After that, a URL's first response is
  enough. A URL whose responses differ refuses the variant: the wrapper depends on something other
  than the request (time, a visitor's cart).
- **Bounded:** a variant keeps 1,000 URLs' wrappers; the oldest learned go first.
- **`notFound()`, `redirect()`** in a page's data function aren't failures: that request goes to
  Next.js, unlearned, and the route goes on (they used to refuse the whole route).

## Options considered

- **Compile layouts too,** so pathname-dependent parts are islands. Right in the long term (it also
  saves the layout's render), but a larger step: layouts are in the module graph but not linked.
- **Keep refusing.** Every app with an active-link menu in its layout would fall back entirely.

## Consequences

- Routes whose wrapper depends on the URL are served after one Next.js response per URL, as with
  islands and per-request metadata. Routes whose wrapper is shared stay served for any request.
- Memory: up to 1,000 documents per variant (Commerce's are ~25 KB).
- Served wrappers can go stale if the data behind them changes: re-verification sampling (step 8).

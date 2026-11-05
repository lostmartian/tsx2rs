# ADR-0018: Next.js's wrapper is captured from real responses, then confirmed

- **Status:** Accepted
- **Date:** 2026-11-06

## Context

A compiled page is only part of a response. Next.js wraps it in a document, its layouts' output,
the router state tree, the reveal runtime and the metadata (ADR-0005, section 9). The runtime must
write that wrapper byte for byte too. ADR-0017 planned to capture it in `next build`, as the
spikes did by hand from one real render.

At build time, a dynamic route's render needs real params and data. With placeholder params,
pages call `notFound()`, which produces another wrapper. Only some routes have
`generateStaticParams`, and anything else would need configuration. The aim is a capture that
works for every route.

What Next.js 16.4.3 writes (the adapter's test app, `next start`):
- **The same request twice** gives the same bytes.
- **Between two params,** the only differences are the page's own output and the param in the
  router state:
  - the page's HTML
  - one Flight row, equal to the engine's model
  - the segment key (`"k":"3"`)
  - the URL (`"c":["","q","3"]`)
- **The query string** is in the router state twice: `"q":"?x=1"`, and after the last segment of
  `"c"`, which is the path and query split on `/`.
- **The page gets its params undecoded** (`a%20b%26c`), and that same string is in the router
  state.
- **The Flight payload** is written as `<script>self.__next_f.push([1,"…"])</script>`, encoded
  with `htmlEscapeJsonString(JSON.stringify(…))`. A non-streaming response is one such script,
  even at 25 KB.

## Decision

**The runtime captures the wrapper from responses Next.js serves anyway:** a route the runtime
can't serve yet falls back. The algorithm is `@tsx2rs/runtime/capture`, and step 8 wires it in.

- **Learn** from one response, given the request's params, path and query string, and the
  *fragments* the runtime rendered for it: the page's HTML and its Flight row (later, metadata as
  well).
  - The Flight scripts are decoded into units. A script that doesn't re-encode to the same bytes
    is refused.
  - Each fragment must be found verbatim, **exactly once**, and becomes a slot.
  - The router state's URL (`"c"`, `"q"`) becomes a slot. These two fields are Next.js internals,
    pinned with the supported version (ADR-0005, section 10).
  - Each place a param's value appears, written as HTML text, JSON, or either of those
    URI-encoded, becomes a **candidate**.
- **Confirm** with a response for different params. It must have the same structure, and every
  byte must match:
  - a candidate becomes a slot if that response has its own value there, and stays literal if it
    has the learned one;
  - matching backtracks, so a value that is also part of nearby text (`"h":16` against a param
    `1`) is told apart;
  - anything else that differs, such as a cookie-dependent layout or a nonce, **refuses the
    route**, with the reason.
- **Fill** writes the document for a request's values, encoding the Flight scripts as Next.js
  does. Until a candidate is confirmed, its param can only take the learned value.

The test is byte-exact round trips on real documents. A template learned from `/q/3` and
confirmed with `/q/7` reproduces responses for a 40-digit param, a URL-escaped param and a query
string with `&` and `%3C`.

## Options considered

- **Capture at build time, with sample params** from `generateStaticParams` or configuration.
  It needs real data at build time, and routes without samples would fall back. Rejected.
- **Capture at build time, with the page stubbed** by the loader. It rebinds user code inside
  Next.js's bundle, and layouts and metadata still need real params. Rejected.
- **Find param places by searching for their values** without a confirming response. A short
  value (`3`) also appears in chunk names and numbers. Rejected: only a second response tells a
  param's place from the same characters.

## Consequences

- The first responses of a route shape are Next.js's: one to learn, and one with different
  params to confirm. Requests with the learned params can be served in between.
- "Identical or fallback" (ARCHITECTURE.md invariant 1) is checked on real responses, request by
  request.
- **Separate shapes:** bot variants (`htmlLimitedBots`) and client navigations (`rsc`) are
  separate shapes, each with its own template.
- **Streaming:** a streamed page writes several Flight scripts, one per flush. The units already
  allow that; step 8 adds the boundary fragments.
- **Not yet:** catch-all params (`string[]`) and binary Flight chunks are refused until a fixture
  covers them.

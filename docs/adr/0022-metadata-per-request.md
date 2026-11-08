# ADR-0022: A page's metadata is learned per request when the params don't explain it

- **Status:** Accepted
- **Date:** 2026-11-08

## Context

Next.js resolves a route's metadata (`metadata`, `generateMetadata`, file conventions) into a list
of `title`, `meta` and `link` elements, and writes it twice (Next.js 16.4.5):
- in Flight, as one row that the head's `Next.Metadata` boundary refers to
  (`{"name":"Next.Metadata","children":"$L12"}`, then `12:[["$","title","0",…],…]`). The list can
  end with Next.js's own client element `IconMark`, which writes no HTML on the server;
- in HTML, as one run of tags in `<head>`, one per host element, in the same order.

The wrapper (ADR-0018) is confirmed with a second response for other params. Metadata made from
the params (`title: "Item " + n`) is explained by param slots, as before. Metadata made from data
(a product's title, a CMS page's description) isn't, so the route was refused: on Commerce, two CMS
pages differ in their metadata.

ADR-0005 planned running Next.js's own metadata code per request. That needs Next.js's request and
work stores, a large internal surface to keep identical across versions.

## Decision

- **Metadata is a fragment of a request when the params don't explain it.** The runtime finds it
  in each response Next.js answers (`findMetadata`): the row the `Next.Metadata` boundary refers
  to, and the one run of the host elements' tags in the HTML, each exactly once. It caches it by
  request (variant and URL), like islands (ADR-0020).
- **The wrapper keeps metadata while the params explain it.** A route's template is learned with the
  metadata as wrapper text. Only if confirming fails does it relearn from the first response with
  the metadata as a fragment, and confirm again. Then a request is served only when its metadata
  is learned; the first request for a URL goes to Next.js and teaches it.
- **Anything else refuses the route**, as before: metadata not found exactly once, or a wrapper
  that differs in ways the params and fragments don't explain.

## Options considered

- **Run Next.js's metadata code per request (ADR-0005).** No first visit per URL, but it depends on
  Next.js's request context internals. It can replace learning later, as an optimisation, without
  changing the template.
- **Always learn metadata per request.** Simpler, but almost every app has metadata (at least the
  root layout's), so every new URL would need a Next.js visit, even on routes that are served for
  any params today.

## Consequences

- Routes with data-made metadata are served after one Next.js response per URL, as with islands.
- Cached metadata can go stale if the data behind it changes: served routes need re-verification
  sampling (step 8), as captured wrappers and islands do.
- On Commerce, the CMS pages are still refused: the layout's footer marks the active link by
  pathname, which is the next case to handle.

# ADR-0019: The runtime server hands everything it doesn't serve to Next.js's own request handler

- **Status:** Accepted
- **Date:** 2026-11-06

## Context

Milestone 1's step 8 is the server that serves compiled routes. The spikes routed every request
themselves, with `@next/routing` and each route's own handler. Then they reimplemented what
`next start` does around those handlers, piece by piece: static files, public files, image
optimization.

Doing that in the product, on Next.js 16.4.3, turned up more of the same:
- public files (`public/`) aren't among the adapter's outputs;
- an `rsc` request without `?_rsc=` must get `next start`'s 307, with its cache-busting hash;
- a URL that happens to end in `.rsc` is a page with that param, not Next.js's internal output.

Each of these, and middleware, i18n, ISR and image optimization besides, is a chance to drift
from `next start`.

## Decision

- **Fallback is `next start` itself.** The server creates Next.js's documented custom server
  (`next({ dir })`, then `getRequestHandler()`) and gives it every request tsx2rs doesn't serve,
  untouched. Fallback is then byte-identical to `next start` by construction, and so is its cost.
- **tsx2rs decides with Next.js's own resolver.** `@next/routing`'s `resolveRoutes`, given the
  build's routing rules, base path, i18n and pathnames, says which route a request resolves to.
  The manifest (version 2, `@tsx2rs/adapter`) carries all of that from `onBuildComplete`.
  Only a GET or HEAD page load of a compiled route is tsx2rs's.
- **Everything goes to Next.js when:**
  - the installed or the building Next.js isn't the supported version (16.4.3);
  - the app has middleware (`proxy.ts`), which would have to run first;
  - the request is a client navigation (`rsc`) or isn't a GET or HEAD.
- **Compiled routes are served in step 8.3,** from the manifest and the captured wrapper
  (ADR-0018). Until then, every request goes to Next.js.

## Options considered

- **Route every request ourselves,** calling Next.js's per-route handlers and reimplementing the
  rest, as the spikes did. It's faster for fallback in theory, but it's a second implementation
  of `next start`'s request handling, which is exactly what tsx2rs must not drift from. Rejected.
- **Put tsx2rs inside Next.js** (a route handler or `proxy.ts`). Phase 0 measured that ceiling at
  about 6x, because Next.js's pipeline still runs. Rejected (D-008).

## Consequences

- The test compares the tsx2rs server with `next start` on the same build, byte for byte:
  pages, query strings, a prerender, a 404, a public file, a static chunk and the `_rsc`
  redirect.
- Fallback requests cost what `next start` costs. Only compiled routes skip Next.js's pipeline,
  which is where the measured gains are.
- Middleware support for compiled routes is open work.

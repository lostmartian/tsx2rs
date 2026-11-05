# ADR-0020: Client component islands are learned from Next.js's responses

- **Status:** Accepted
- **Date:** 2026-11-06

## Context

In Commerce, `/`, `/product/[handle]` and `/product-streaming/[handle]` fall back because a page
renders a client component element (`next/link`, `next/image`, the app's own `"use client"`
components). React renders a client component on the server too: its HTML goes into the page, and
Flight gets an `I` row with the module's id and chunks, which the page row references
(`["$","$L15",null,{...props}]`, seen on Next.js 16.4.3).

Neither part is something tsx2rs can compute at build time without running the client module:
- the HTML is whatever the client component renders for its props;
- the row id is assigned in the order React meets references, and the `I` row's chunk list comes
  from Next.js's client manifest.

## Decision

- **The compiler lowers a client component element to an island.** The props and children are
  compiled as usual: children are native HTML between two `Op::Island` holes (the island's HTML
  before and after them), and Flight is `["$", <reference>, key, {...props, "children": ...}]`.
  The reference is an `Op::Value` with the new input tag `REFERENCE` (11): a string written as
  is, without React's `$` escape, and only `$`, an optional letter or `@`, then lowercase hex,
  so input can't break out of the row.
- **The runtime learns each island's HTML and reference from Next.js.** It renders with
  placeholder markers (`<!--tsx2rs:N:a-->` and reference `$Ltsx2rsN`), finds what Next.js wrote
  in the gaps around the native parts, and caches it. The cache key is the component, its props,
  its ordinal in the page, the variant (ADR-0018), and the pathname with the search. A miss goes
  to Next.js, which is also the learning request.
- **Until the runtime handles a case, it stays a Child hole (React renders it):** an island at
  a component's root, an island with spread props, an island with JSX props other than
  `children`.

## Options considered

- **Run client components in Rust.** They're arbitrary code with hooks; it's the whole V8
  problem again.
- **Render islands with React per request (Child holes).** Correct, and the current fallback,
  but every page with a `Link` would pay for React.
- **Compute the `I` row from Next.js's client manifest.** Possible for the row, but not for the
  HTML, and the id ordering is React's internal detail. Learning both from the response keeps
  identical-or-fallback (ADR-0002) without depending on it.

## Consequences

- Pages with client components can be served compiled after a learning request per cache key.
  Keys that include the URL mean a route with many URLs learns per URL: a later ADR can narrow
  the key once islands are shown not to depend on the URL.
- A cached island can go stale when the client component changes behaviour over time (dates,
  random values): served routes need re-verification sampling, as captured wrappers do.
- Input format: tag `REFERENCE` is new; programs that don't use it are unaffected.

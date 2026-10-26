# ADR-0010: Hoisting into `<head>`: the engine collects React's queues, the runtime places them

- **Status:** Accepted
- **Date:** 2026-10-27

## Context

ADR-0009 rendered the in-place forms of `<title>`, `<meta>`, `<link>`, `<script>` and `<style>`,
and left the hoisted ones falling back. React DOM's server (react-dom-server 19.3.1) hoists these
into per-request queues in `renderState` and writes them into the document's `<head>`
(`flushCompletedQueues`), in this order:

1. charset `<meta>`
2. preconnects
3. viewport `<meta>`
4. font preloads
5. high-priority image preloads
6. styles by precedence
7. import maps
8. bootstrap scripts
9. `async` scripts
10. bulk preloads
11. the other hoisted elements
12. then the `<head>` element's own children

In Next.js the document (`<html>`, `<head>`, `<body>`) comes from the root layout. The runtime
writes it from the captured wrapper (ADR-0005, section 9), and the page's hoisted elements must
end up in that `<head>`, deduplicated against what Next.js puts there itself.

## Decision

### The engine collects

`Output` gains a `Head`: React's queues for what elements hoist, filled during the render and
kept across renders for one request, cleared by `Output::clear`.

- **How an element gets there.** An element React hoists is rendered exactly as in place, then
  moved from the HTML output into its queue. That moved form is identical to React's: React
  writes hoisted elements with the same functions it uses in place (`pushTitleImpl`,
  `pushLinkImpl`, `pushSelfClosing`, `pushScriptImpl`).
  - **The text separator.** Where text came just before, the body keeps React's `<!-- -->`
    separator for every hoisted element except `<title>`.
- **Where each element goes:**
  - **`<title>`, and `<link>`s that aren't stylesheets:** the hoisted-element queue.
  - **`<meta>`:** the charset queue when `charSet` is a string, the viewport queue for
    `name="viewport"`, otherwise the hoisted-element queue.
  - **`<link rel="stylesheet" precedence>`:** its precedence's group. `precedence` is dropped
    and `data-precedence` set to it: in the place of a `data-precedence` prop, else last.
  - **`<style href precedence>`:** its content joins the group's rules and its `href` the
    group's `data-href`, written as one `<style data-precedence data-href>` after the group's
    stylesheets. It's left out when the group has only stylesheets.
  - **`<script async src>`:** the scripts queue.
- **Image preloads.** An `<img>` React preloads still renders in place, and its
  `<link rel="preload" as="image">` is built from the props as React builds it (`href` or
  `imageSrcSet` and `imageSizes`, `crossOrigin` mapped to `""` or `"use-credentials"`,
  `integrity`, `type`, `fetchPriority`, `referrerPolicy`). The first ten, and every
  `fetchPriority="high"` one, go to the high-priority queue; later ones go to bulk preloads.
  A later image with the same key moves a bulk preload to the high-priority queue by the same
  rule (React's `promotablePreloads`).
- **A bigint in a resource hint is an error.** React's Flight server emits a resource hint for a
  preloaded image and for stylesheet, preload and modulepreload links, and `JSON.stringify` throws
  on a bigint in it. The engine returns `InvalidValue`, as React's Flight turns the element into
  an error.
- **Deduplication, as React's resumable state:**
  - stylesheet and style `href`s, shared between the two
  - classic and module script `src`s, separately
  - image preload keys: `src`, or `srcSet` + newline + `sizes`
  - A duplicate `<style>` is skipped entirely, including React's content checks.
- **`Head::write`** writes the queues in React's order. Preconnects, font preloads, import maps
  and bootstrap scripts don't come from elements (they come from React DOM's resource APIs and
  from Next.js), so they aren't the engine's.

### The runtime places

The runtime (steps 7–8) writes `Head` into the captured document's `<head>`, at the positions
React would, around Next.js's own queue contents. It also seeds the deduplication with what the
document already has (Next.js's stylesheets, scripts and preloads). That's part of capturing the
wrapper. Two Next.js specifics are verified there, not here:

- whether Next.js passes React's `onHeaders`, which sends image preloads as HTTP `Link` headers
  instead
- a style `nonce` (CSP), which makes React drop non-matching `<style>` resources

### Tested against whole documents

The differential test now renders every tree as the body of
`<html><head></head><body>…</body></html>`, on both sides, and compares the whole document. So
hoisting, order and deduplication are checked against React for every generated tree.

## Options considered

- **Write hoisted elements through a separate path** (React's `push*Impl` into a queue). Same
  output, but it duplicates every attribute and content rule. Rejected: rendering in place and
  moving the bytes reuses the rules, at the cost of a copy per hoisted element.
- **Leave hoisting to the runtime** (the engine reports hoistable elements, and the runtime
  writes them). The runtime would need React DOM's attribute rules. Rejected: escaping and
  attributes belong to the engine (ARCHITECTURE.md invariant 5).

## Consequences

- No element makes a route fall back for hoisting any more. What remains are the `Suspense`
  fallback scope (hoistables dropped in fallbacks, with Suspense) and the merge with Next.js's
  `<head>`.
- **Known gaps:** a stylesheet `<link>` with children makes React's own `renderToString` write a
  broken document, where the engine reports an error. And a `data-precedence` prop that is
  `null` keeps its key's position in React; the engine appends.
- `Output` is bigger, and `Output::clear` resets the deduplication: one `Output` per request.
- **Flight resource hints are the next step.** React's Flight server emits hint rows for the same
  elements (`:HL[href, "image" | "style", options]` for preloaded images and for stylesheet,
  preload and modulepreload `<link>`s, `:Hm` for module preloads), deduplicated per request.
  It does so for compiled templates' static `<img>` and `<link>` elements too. The engine
  doesn't write them yet, and the differential test compares Flight's element tree, not its
  hint rows. Testing this pull request found it, and found that the identity harness decoded
  hints as Flight values instead of plain JSON (fixed here).

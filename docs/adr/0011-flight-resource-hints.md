# ADR-0011: Flight resource hints, and hinted elements always as element values

- **Status:** Accepted
- **Date:** 2026-10-28

## Context

React's Flight server writes **resource hint rows** into the RSC payload while it serializes
host elements (`getChildFormatContext`, `preload`, `preloadModule`, `emitHint` in react-server
19.3.1). The browser's React uses them to start loading images, stylesheets, fonts and module
scripts early. They're written before the flush's other rows, and deduplicated per request. Testing
hoisting (ADR-0010) found that the engine wrote none.

The rules:

- **`<img>`:** when React DOM would preload it (same conditions; Flight's own `<noscript>` and
  `<picture>` context), `:HL[src or "", "image", options]`. The options are `imageSrcSet`,
  `imageSizes` (a string), `crossOrigin` (mapped to `""` or `"use-credentials"`), `integrity`,
  `type`, `fetchPriority` and `referrerPolicy`.
- **`<link>`** outside `<noscript>`, without `itemProp`, with a string `rel` and a non-empty
  string `href`:
  - `preload` → `:HL[href, as, options]`
  - `stylesheet` → `:HL[href, "style", options]`
  - `modulepreload` → `:Hm[href, options]`, or `:Hm"href"` without options
- **Options:**
  - in React's fixed order, not the props' order
  - `null` and `undefined` are left out (`trimOptions`), and the array has no options element
    when none is left
  - the JSON is plain `JSON.stringify`: no `$` escaping, `undefined` and non-finite numbers
    become `null`, and a bigint throws
- **Deduplication keys:** `L[image][srcSet][sizes]` or `L[image][][]href` for images (shared
  with `<link rel="preload" as="image">`), `L[as]href` for other preloads (a stylesheet is
  `L[style]href`, with or without `precedence`), and `m|href` for module preloads.

## Decision

- **`Output` gains `hints: Hints`:** the hint rows, in emission order, deduplicated across renders
  for one request and reset by `Output::clear`. The runtime writes them before the Flight rows of
  the same flush.
- **The engine emits a hint where it writes an element value's Flight,** before the element's
  children, as React does. Inside `<picture>`, React's Flight server moves the children to their
  own row, but still emits their hints in document order, so emitting as the engine goes matches.
- **A bigint in a hint's props is an error** (`InvalidValue`; ADR-0010), because React's Flight
  turns the element into an error.
- **Where the engine writes an element to Flight only** (the children of `<textarea>`,
  `<script>`, `<title>`, `<style>`), an `<img>` or `<link>` falls back (`UnsupportedElement`):
  React would emit its hint, and that path doesn't read props.
- **The compiler lowers every element that hoists or hints as an element value:** `<title>`,
  `<meta>`, `<link>`, `<script>`, `<style>` and `<img>`, never as static template markup. Then
  HTML hoisting (ADR-0010) and Flight hints both come from the engine's element path, with no new
  ops. These elements are few per page, so the cost of element values is small.
- **The differential test compares the hint rows too,** in order, along with the element tree.

## Options considered

- **A `Hint` op for static elements in templates.** A compiled `<img src="/a.png">` could carry a
  precomputed hint row and dedupe key. But the element's HTML side (preloads in `<head>`, and
  hoisting for the other kinds) would need ops too, and the context (`<noscript>`, `<picture>`)
  is known only at runtime once templates are called (ADR-0009). Rejected: one path for these
  elements is simpler, and correct by construction.
- **Leave hints out.** The decoded element tree is identical without them, but the payload
  isn't, and the browser would start loading these resources later than with Next.js. Rejected
  under ADR-0002.

## Consequences

- With this, step 5's engine side covers what React DOM and React's Flight server do for every
  host element. What remains is in the runtime: placing `Head`, and merging hints with
  Next.js's own (fonts, for example) on the same deduplication.
- Hint rows are a third output the runtime has to place.

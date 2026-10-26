# ADR-0009: Hoistable elements rendered in place, and the document's elements inside the body

- **Status:** Accepted
- **Date:** 2026-10-26

## Context

ADR-0007 made the engine return `UnsupportedElement` for `<title>`, `<meta>`, `<link>`,
`<script>`, `<style>`, `<html>`, `<head>` and `<body>`, until a hoistables pull request. But
React DOM (`pushStartInstance`, react-dom-server 19.3.1) writes many of these *in place*. One of
them is the commonest real use: an inline `<script type="application/ld+json">`, which Vercel
Commerce's product page renders in its body. Only some cases are hoisted into `<head>` or
collected as resources.

## Decision

The engine renders every in-place case by React DOM's rules. The hoisted cases still return
`UnsupportedElement` until the hoisting pull request.

- **In place whatever the props:** inside `<svg>` (until a `<foreignObject>` or an element that
  starts an HTML context), inside `<noscript>`, or with an `itemProp`. The engine's element
  context gains React's SVG insertion mode, and separate `<noscript>` and `<picture>` scopes.
- **In place by their props:**
  - `<script>` unless it has a non-empty string `src`, a truthy `async`, and no `onLoad` or
    `onError`. Its content is `dangerouslySetInnerHTML` or a string child, with `<script` and
    `</script` (any case) escaped as React does (`s` / `S`).
  - `<style>` unless it has a string `precedence` and a non-empty string `href`. Its content is
    one child (an array of one counts as its item) or `dangerouslySetInnerHTML`, with `<style`
    and `</style` escaped as `\73 ` / `\53 `.
  - `<link>` unless it has string `rel` and non-empty string `href`, and (for `rel="stylesheet"`)
    a string `precedence` with no `disabled`, `onLoad` or `onError` (for other `rel`s, no
    `onLoad` or `onError`).
  - `<title>` and `<meta>` only in the contexts above.
- **Content rules:**
  - `<title>`'s one child is written as escaped text.
  - `<title>`, `<script>` and `<style>` don't render their children as nodes, though Flight still
    serializes them. A compiled component among those children falls back
    (`UnsupportedElement`): its Flight comes only from running its template, which would apply
    React DOM's checks where React makes none.
  - `<meta>` and `<link>` are self-closing, so content is an error.
- **`<html>`, `<head>` and `<body>` inside the body are generic elements**, as React writes them
  outside its root and HTML-document modes. A `Child` node renders in the document body's
  context (ADR-0007): the document's own `<html>`, `<head>` and `<body>` come from Next.js's
  wrapper (ADR-0005, section 9).
- **The differential test checks declined elements exactly.** When the engine returns
  `UnsupportedElement`, React must have hoisted something: rendered as the body of a document,
  React's `<head>` must not be empty.

## Consequences

- Inline scripts (JSON-LD), structured-data `<meta itemProp>`, SVG `<title>`s and plain
  stylesheet `<link>`s render natively.
- **Context flows through components, as in React.** A template runs in the context of its call
  site (`Op::Call`, template nodes), and its `Child` holes use that context. So a component
  inside an `<svg>`, `<noscript>` or `<select>` element value gets React's context: SVG mode,
  the noscript scope, the select's value.
- **The compiler lowers a context-changing element with anything runtime inside as an element
  value** (`<svg>`, `<noscript>`, `<picture>`, `<math>`, `<select>`, tables). Then every
  `Child` hole sees exactly the context React would. A hole inside such an element in compiled
  markup would start in its template's context, which is wrong. The differential test lowers
  elements this way.
- Hoisting (`<title>`, `<meta>`, `<link>` in the body), resources (stylesheets with
  `precedence`, `async` scripts) and image preloads are the next pull request, with their own
  ADR: per-request queues written where Fizz writes them in `<head>`, deduplicated as React does.

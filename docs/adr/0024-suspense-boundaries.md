# ADR-0024: Suspense boundaries are compiled, and their form is learned from Next.js

- **Status:** Accepted
- **Date:** 2026-11-08

## Context

Commerce's product page wraps its gallery and description in `<Suspense>`; until now a boundary
was a hole React rendered, so the page fell back. What Next.js 16.4.5 writes for a boundary
(`/product/acme-hoodie`, `next start`):

- **Flight:** React's element tuple, `["$","$b",null,{"fallback":…,"children":…}]`, where `$b`
  refers to the `"$Sreact.suspense"` row.
- **HTML, resolved:** `<!--$-->`, the children, `<!--/$-->`.
- **HTML, pending:** in the shell, `<!--$?--><template id="B:1"></template>`, the fallback,
  `<!--/$-->`; after the shell, `<div hidden id="S:1">`, the children, `</div>`, and React's reveal
  script `<script>$RC("B:1","S:1")</script>` (the first one also defines the reveal runtime).

Which boundaries are pending isn't in the source: on this page, the gallery and description are
pending (their client components load during the render) while the layout's cart boundary
resolves. The `B:`/`S:` numbers count the document's boundaries, layout's included.

## Decision

- **The compiler lowers `<Suspense fallback={…}>` to a boundary** (`Slot::Suspense`), when it isn't
  a component's root and has no props but `fallback`. Flight is the element tuple, its reference
  an `Op::Value` (`REFERENCE`, ADR-0020). Its fallback and children are compiled as usual.
- **The engine gets HTML outputs** (`Op::HtmlOutput`, `Op::EndHtmlOutput`): the ops between them
  write HTML inline, to a second output (`segments`, what React sends after the shell) or
  nowhere, as the input says. Flight is unaffected.
- **The runtime gives each boundary its form**: its reference, whether it's pending, and its four
  markers (before and after the fallback, before and after the children), all as `Op::Island`
  strings. A pending boundary's fallback goes inline and its children to the segments output; a
  resolved one's fallback nowhere and its children inline. The runtime learns the forms from
  Next.js's responses, as it learns islands (next PR); without them, the encoder refuses
  (`NotNative`).
- **Not yet:** a pending boundary inside another pending or discarded part, and hoistables
  (`<title>`, image preloads) there, which React writes with the segment. The engine refuses them
  (`UnsupportedBoundary`), so the request falls back.

## Options considered

- **Decide pending vs resolved at build time.** It depends on what suspends during Next.js's
  render (client chunks loading, data), which the compiler can't see.
- **Leave boundaries to React (a `Child` hole).** Then every page with a boundary pays for React,
  which is most Next.js pages.

## Consequences

- Commerce's product page: 14 ops before, 92 now; the gallery and the description are compiled.
- The segments are a second HTML fragment of the page, which the wrapper is captured around.

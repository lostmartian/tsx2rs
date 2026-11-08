# ADR-0025: The engine splits Flight into rows as React does, and async components are compiled

- **Status:** Accepted
- **Date:** 2026-11-08

## Context

The engine renders a page's Flight as one value, one row. Commerce's product page, as Next.js
16.4.5 writes it, has more rows than that:

- each async server component's output is a row of its own (`RelatedProducts` → `$L23`, the
  `Footer` → `$L24`);
- inside those, list items and a `Label` are rows of their own (`2b`–`30`): React's Flight server
  (`react-server-dom-*`, production) keeps a running `serializedSize` per task (the length of
  every property name and array index it visits, and of every string), and when it reaches an
  element or a server component's element while that size is over 3200, it defers it to a new
  task (`deferTask`), so a new row;
- strings of 1024 characters or more become text rows (`$T`).

The differential tests compared decoded Flight *trees*, so this never showed; serving needs the
rows byte for byte (ADR-0018).

## Decision

- **The compiler marks where React checks** (`Op::Mark`): before each server component's output
  (`Component`), and before an async one's (`Async`). The engine records the marks' positions,
  including a compiled component reached through an `Op::Child` hole.
- **The engine splits the value as React does** (`outline`): it replays React's walk, counting
  what React counts (property names, array indices, strings as React had them: `$$x` was `$x`, and
  React's own `$` strings for non-strings don't count), and defers an element or a marked
  component past 3200, and an async component always, to a row of its own. Rows come out in
  React's task order; a reference to one is `"$L!<n>"`, for the runtime to replace with the id
  Next.js gave it.
- **Async server components are linked.** Their data functions return promises; the generated
  encoder awaits them (`callAsync`) and is async only for pages that have one.
- **Not yet:** text rows. A string of 1024+ characters refuses the request. The runtime serves only
  single-row pages until it learns row ids (next PR); others are refused with that reason.

## Options considered

- **Split at build time.** The sizes depend on the data (a product's title, a list's length).
- **Post-process the JSON without marks.** A server component's element leaves no trace in the
  output, and React checks there too; the marks are what make the replay exact.

## Consequences

- A differential test renders large generated trees (up to 200 keyed items, components, non-ASCII
  text) with React's own Flight server and compares the raw rows: byte-identical, nested deferrals
  included.
- Commerce's product page: 92 ops before, 267 now (`RelatedProducts` and the `Footer` compiled).

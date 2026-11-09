# ADR-0030: Flight rows are split in one streaming pass

- **Status:** Accepted
- **Date:** 2026-11-09

## Context

`outline` splits a rendered Flight value into rows as React's Flight server does (ADR-0025) and
writes repeated objects as references (ADR-0026). It parsed the whole value into a tree of nodes,
indexed every node's start in a hash map, then walked the tree and wrote every byte again. On
Commerce's product page it took 1,457,198 instructions, 4.4 times the render itself
([#86](https://github.com/lostmartian/tsx2rs/pull/86)). Profiling `tsx2rs start` showed the native
call at about 31% of the CPU, mostly `outline`. On Google Cloud tsx2rs serves 18–26% of the
hand-written spike's throughput (lab, D-055).

The plan was to split rows while rendering. That isn't practical yet. A program's static Flight is
precompiled byte chunks, and the engine doesn't know their structure at render time. The size
React counts, and so where it defers, depends on both the static chunks and the dynamic values
between them.

## Decision

- **`outline` reads the value once, without building a tree.** A row is its source span with the
  deferred values (`"$L!<n>"`) and references (`"$!<row>:<path>"`) put in place, so everything else
  is copied in bulk.
- **A deferred value is skipped where it is** (its end found by a bracket scan) **and read when its
  row's turn comes**, in React's task order, as before.
- **Marks are read in order** with a cursor, instead of a hash map of every node. The render already
  records them in order; marks out of order are sorted first.
- **String ends are found with `memchr`'s SIMD search.** Past React's 3200 limit a string's exact
  UTF-16 length no longer changes any decision, so bytes are counted instead, and a string is
  counted exactly again if its byte count reaches a text row's 1,024.
- **The first implementation is kept for tests only**, as the reference: a property test checks
  the two give the same rows (or the same error) on 2,000 random values with random marks. About a
  third of the values split into several rows and a quarter have references. A unit test checks the
  same on Commerce's real page.

## Consequences

- The same rows, byte for byte.
- `memchr` becomes a direct dependency of the engine. It was already in the tree, through napi.
- On Commerce's product page, laptop wall-clock: about 120–145 µs against 275–305 µs before. CI's
  instruction count (`outline_rows/commerce_product`) gives the exact change.
- Splitting while rendering stays possible later: it needs the compiler to give each static Flight
  chunk its structure (its size contribution and where its elements start).

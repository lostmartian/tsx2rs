# ADR-0026: Objects React has written before are written as references

- **Status:** Accepted
- **Date:** 2026-11-08

## Context

React's Flight server keeps every object it writes in `writtenObjects`, with the path where it
wrote it: the row's id, then each property name, array index, or an element's `type`, `key` or
`props` (`$a:1:props:children:0:props:options`). When it reaches the same object again, it writes
that reference instead of the object. On Commerce's product page, `product.options` goes to
`VariantSelector` and, inside `product`, to `AddToCart`: Next.js writes the second as
`"$a:1:props:children:0:props:children:1:props:children:props:children:1:props:options"`. The
engine wrote it twice. (A path is registered only while its parent's is, and not under a key with
a `:`.)

## Decision

- **The encoder tags each array and plain object it writes as a value** with its identity in the
  input (`tag::IDENTITY`: a number per object, per render). Element nodes' own props aren't tagged
  (the engine's element writer reads them).
- **The engine marks where each identified object starts**, and `outline` (ADR-0025) registers
  every object's path as React's walk reaches it, in React's task order, and writes a later
  occurrence of an identity as `"$!<row>:<path>"`. Which occurrence is first is React's walk's
  business, not the encoder's: deferral changes the order.
- **The runtime learns the page row's id** with the other rows' (`"$!0:…"` needs it), and fills
  both kinds of placeholders per request.

## Consequences

- Commerce's product pages, streamed and not, are served compiled and byte-identical once each URL
  has been learned: all 9 of milestone 1's reference pages.
- Objects shared through `style` and spread props of host elements aren't identified yet; a page
  where they repeat doesn't learn, and stays Next.js's.

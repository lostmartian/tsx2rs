# ADR-0033: The engine gzips served pages for the usual gzip request

- **Status:** Accepted
- **Date:** 2026-11-09

## Context

`next start` compresses responses with Next.js's `compression` middleware
(`next/dist/compiled/compression`): a new Node.js zlib stream per response, at zlib's default
level 6. tsx2rs applied the same middleware to the pages it serves.

On Google Cloud (lab D-059), gzip cost tsx2rs 11–13% of its throughput at 1 process and **43–47% at
8** on Commerce's product pages (Next.js: 0–8% and 13–17%). Profiling (lab D-058) showed:
- gzip's own work (`deflate_slow`, `longest_match`) at about 4.5 times the samples of the whole
  tsx2rs engine;
- each stream's setup and teardown: zlib's ~256 KiB of state allocated and zeroed, and
  `ZlibBase._destroy` on the main thread, about 2.4%.

This is the first of two steps. The second compresses a template's fixed parts once (a later ADR).

## Decision

- **The engine has a gzip compressor** (`tsx2rs_engine::Gzip`, on `flate2` with the `zlib-rs`
  backend):
  - a raw deflate stream at level 6 in a gzip member;
  - its state reset and reused between documents, one per thread;
  - a document given in parts is compressed as one.
- **napi's `gzip(parts)` runs it on libuv's thread pool** and returns a Promise, as Node.js's zlib
  does, so the main thread doesn't compress.
- **A served page goes through the engine when the middleware would gzip it, in the usual case:**
  - an `Accept-Encoding` without weights (no `;`) that names `gzip` (what browsers and `oha` send);
  - a `GET` (not `HEAD`);
  - `text/html`, at least the middleware's 1 KiB threshold;
  - no `Content-Encoding`, no `no-transform` in `Cache-Control`.

  The response gets `Content-Encoding: gzip` and `Accept-Encoding` added to `Vary` as the
  middleware's `vary` adds it.
- **Everything else still goes through the middleware itself**: weights, `*`, deflate-only clients,
  `HEAD`, other types. Its decisions stay exact without reimplementing its content negotiation.

## Consequences

- The same responses as `next start` in every header the middleware sets and in the decoded body.
  A test runs Next.js's real middleware next to the route for 14 `Accept-Encoding` values, as `GET`
  and `HEAD`, and compares `Content-Encoding`, `Vary` and the decoded body. It also checks which
  compressor made each response. The compressed bytes differ from zlib's (another implementation,
  and the gzip header's OS byte is "unknown"); their size is the same here (11.6 KB on the T-shirt
  page).
- On the laptop at 1 process, gzip no longer costs throughput (243–247 rps with it, 241–249
  without; before, about 9% less with it). At several processes compressing still costs CPU: the
  next step (fixed parts compressed once) addresses that.
- `flate2` and `zlib-rs` (MIT/Apache-2.0 and Zlib, already allowed) become engine dependencies.
  They're pure Rust, so they need no C toolchain.

## Options considered

- **A lower gzip level:** less CPU, bigger responses than `next start`'s. Not chosen: the user
  preferred keeping `next start`'s output size.
- **Compressing synchronously on the main thread:** simpler, but at 1 process it would take the
  main thread's time, where zlib used idle cores.
- **Reimplementing the middleware's whole negotiation (weights, `*`):** more code to keep exact,
  for rare requests. The middleware still handles them.

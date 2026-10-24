# Architecture

This document describes the high-level architecture of tsx2rs: what lives where, how data flows, and the rules (invariants) that hold everywhere. It describes the target design for milestone 1; components that don't exist yet are marked *(planned)*. Read it before your first change. Decisions behind it are in [`docs/adr/`](docs/adr/).

## Bird's-eye view

tsx2rs is a **Next.js Deployment Adapter**. A Next.js app turns it on with one line (`adapterPath` in `next.config`).

- **At build time:**
  - The adapter compiles each eligible route's server component templates into a **template program**: precomputed static HTML and RSC (Flight) byte chunks plus a few ops.
  - It also generates a **data encoder** for each route.
  - It captures the parts Next.js produces around the page (document, layout payload) as templates with slots.
- **At request time**, the runtime routes the request the same way Next.js does:
  - **Compiled route:**
    1. The app's own data-fetching code runs in Node.js.
    2. The encoder packs the values the template needs into one buffer.
    3. The Rust **engine** runs the route's program and writes HTML + Flight.
    4. Client components come from an HTML cache, rendered by React on a miss.
  - **Anything else:** the request goes to Next.js's own handler, unchanged.

```
                      build time                                          request time
 app/**/*.tsx ──► compiler ──► template programs + encoders + manifest ──► runtime ──► engine ──► HTML + Flight
                   (Rust)       (versioned files)                          (Node)      (Rust)
                                                                              │
                                                                              └──► Next.js handler (fallback)
```

## Code map

### `crates/ir` *(planned)*
The shared vocabulary: template IR, data schema (field shapes the encoder writes and the engine reads), and the **template-program binary format** (magic, version, sections). It has no dependencies on other crates and does no I/O. Both the compiler and the engine depend on it; nothing else defines these types.

### `crates/compiler` *(planned)*
Source → IR → program, in phases with one module each:
1. **Parse** (oxc)
2. **Module graph** from a route entry
3. **Server/client split** (`"use client"`)
4. **Setup/template split** with lifting of non-compilable expressions
5. **Schema inference**
6. **Lowering** to IR
7. **Program emission**
8. **Encoder generation**

Each phase is a pure function of its input. Unsupported constructs produce a **diagnostic** (stable code, message, source span) that makes the route fall back. The compiler never guesses.

### `crates/engine` *(planned)*
Runs template programs: copies static chunks and executes ops against the encoded input. It writes HTML by React's server rendering rules and Flight rows by React's Flight rules. Escaping lives here and only here. It validates every input buffer (bounds, UTF-8) and reports a typed error instead of panicking.

### `crates/napi` *(planned)*
The only bridge to Node.js (napi-rs). Thin: converts arguments, calls `compiler`/`engine`, maps errors. No logic. This is the only crate allowed `unsafe` (FFI), and it catches panics at the boundary.

### `packages/adapter` *(planned)*
`@tsx2rs/adapter`: the Next.js Deployment Adapter. In `onBuildComplete` it runs the compiler, captures each compiled route's document and layout payload, and writes the **manifest** (versioned JSON: routes, programs, encoders, capture templates, fallback reasons).

### `packages/runtime` *(planned)*
`@tsx2rs/runtime`: the HTTP server.
- Routing (`@next/routing`, the same resolver Next.js uses)
- Fallback to Next.js handlers
- Running each route's setup code
- Encoding
- The island cache with context-aware keys
- Streaming of `<Suspense>` boundaries
- Image optimization
- A cluster launcher (one process per vCPU)

### `packages/native` *(planned)*
`@tsx2rs/native`: the compiled napi binary as an npm package (prebuilt per platform after milestone 1).

### `tests/`
- `identity/`: the harness that builds an app with Next.js, runs both servers, and compares output.
- `fixtures/`: small apps, one feature each.
- `apps/`: real apps (Vercel Commerce).

## Data flow of a compiled request

1. The runtime resolves the route (`@next/routing`). If it isn't compiled, or the request is outside the compiled shape (e.g. per-visitor cookie, `rsc` navigation not yet compiled), it goes to Next.js.
2. The route's setup code (the app's own data fetching, unchanged) runs and produces the values the template needs. Lifted expressions are evaluated here.
3. Client components are looked up in the island cache by a key of component, props and the contexts they read. On a miss, React renders them.
4. The generated encoder writes all values into one reusable buffer: **one native call per flush** (ADR-0003).
5. The engine runs the program and writes HTML and Flight into reusable output buffers.
6. The runtime assembles the response from the captured document template and the engine's output, then streams the shell first and each `<Suspense>` boundary when its data resolves.

## Invariants

These hold everywhere. A change that breaks one needs an ADR.

1. **Identical or fall back.** For a compiled route, the HTML (after applying streamed boundaries) and the decoded Flight tree are identical to Next.js's. When the compiler or runtime can't guarantee that, the request goes to Next.js. An error at request time also falls back (or returns 500 if headers were already sent), never partial wrong output.
2. **Layering:** `ir` ← `compiler`, `ir` ← `engine`, `{compiler, engine}` ← `napi` ← `packages/*`.
   - The engine never depends on the compiler (it can ship alone in Stage B).
   - Packages never reach into crate internals.
   - No dependency cycles.
3. **Versioned boundaries.** The program format and the manifest carry a version. A runtime refuses (and falls back on) a version it doesn't understand.
4. **No global mutable state.** Configuration is passed in explicitly (handles, not statics), so multiple apps or routes can coexist in one process and tests can run in parallel.
5. **Escaping in one place.** Only the engine turns data into HTML/Flight text. Nothing upstream pre-escapes; nothing downstream re-escapes.
6. **Panics never cross the FFI boundary.** `crates/napi` catches them and turns them into errors, which become a fallback.
7. **Deterministic builds.** The same input produces the same programs and manifest, byte for byte.

## Cross-cutting concerns

- **Errors:**
  - Rust: library crates use typed errors (`thiserror`).
  - JS: errors carry a stable `code`.
  - Compiler diagnostics have codes such as `TSX2RS_E0001`, each documented.
- **Observability:** the runtime counts compiled vs fallback requests (by reason), island-cache hits and misses, and engine time. It logs structured JSON.
- **Performance:** hot paths are the engine, the encoder and the island-cache lookup. Changes there need benchmark numbers in the PR ([testing](docs/engineering/testing.md#performance)).
- **Security:** all request data is untrusted. The engine validates every input buffer, the image optimizer validates paths and sizes, and dependencies are checked by `cargo deny` and `pnpm audit`.

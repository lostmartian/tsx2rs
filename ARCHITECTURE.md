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

### `crates/ir`
The shared vocabulary.
- **`Program`:** a chunk table plus a table of templates (one per compiled component that isn't inlined, template 0 the entry, [ADR-0008](docs/adr/0008-template-calls.md)), each a list of semantic ops: static output, text, number, props (React DOM's attribute rules, for names known at compile time and for props objects known only at runtime, `ir::attribute`), `style` objects, raw HTML, generic values, runtime-typed children (text, arrays, element values and compiled components known only at runtime, [ADR-0007](docs/adr/0007-element-values.md)), islands, loops, branches, and calls to other templates.
- **Its binary format:** magic, version, chunks, ops.
- **The input format** the generated encoder writes (`ir::input`): values in program order, as length-prefixed UTF-8, `f64`, tags and counts.

`Program::new` validates every structural rule the engine relies on, and decoding goes through it. The crate has no dependencies on other crates of ours and does no I/O. The compiler and the engine both depend on it, and nothing else defines these types. *(The template IR and data schema used inside the compiler are planned.)*

### `crates/compiler`
Source → templates + data functions → programs, in phases with one module each ([ADR-0005](docs/adr/0005-compiling-server-components.md)):
1. **Parse** (oxc)
2. **Module graph** from a route entry
3. **Component usage:** server, client and React built-in components (`"use client"`, re-exports)
4. **Templates:** JSX in template positions becomes a template; other JSX stays real JSX and becomes an element value at runtime *(planned)*
5. **Data functions:** span-based rewriting of the component's own code, with each returned JSX replaced by its hole values *(planned)*
6. **Linking:** child components, slots, and `Call` or template nodes for recursion and runtime components *(planned; the engine side exists)*
7. **Lowering** to programs, and the **encoder** from the same traversal *(planned)*

Each phase is a pure function of its input. Its **diagnostics** have a stable code, a message, a source span and a severity: an *error* makes the route fall back, and a *note* names a part that React renders at request time. The compiler never guesses.

### `crates/engine`
Runs template programs.
- **One pass, two outputs:** it walks the ops once, reading each input value once, and writes HTML by React DOM's server rules and Flight by React's Flight rules at the same time.
- **Escaping lives here and only here:** text and attributes, Flight strings, and Flight for inline `<script>` tags (`script_string`).
- **Hoisting** (`head.rs`, [ADR-0010](docs/adr/0010-hoisting-into-head.md)): what React DOM hoists into `<head>` (titles, meta, links, stylesheets by precedence, `async` scripts, image preloads) is collected into `Output::head`, in React's queues and with its deduplication; the runtime places it in the document's `<head>`.
- **Flight resource hints** (`hints.rs`, [ADR-0011](docs/adr/0011-flight-resource-hints.md)): the `:HL`/`:Hm` rows React's Flight server emits for `<img>` and `<link>` elements, deduplicated per request, in `Output::hints`.
- **Element values** (`render/element.rs`): host elements that reach a hole at runtime, written by React DOM's rules for their type (`input`, `select`/`option`, `textarea`, custom elements, in-place `<script>`/`<style>`/`<title>`/`<meta>`/`<link>`, …), with the context their descendants need (the enclosing `<select>`'s value, SVG mode, `<noscript>` and `<picture>` scopes).
- **Input is untrusted:** it is checked as it is read (lengths, tags, UTF-8, nesting depth, leftover bytes), and failures are typed errors, never panics.

### `crates/napi` *(planned)*
The only bridge to Node.js (napi-rs). Thin: converts arguments, calls `compiler`/`engine`, maps errors. No logic. This is the only crate allowed `unsafe` (FFI), and it catches panics at the boundary.

### `packages/adapter` *(planned)*
`@tsx2rs/adapter`: the Next.js Deployment Adapter.
- **During the build,** a bundler transform (registered in `modifyConfig`) adds each server module's generated data functions to Next.js's own bundle, next to the unchanged original code.
- **In `onBuildComplete`,** it links each route's programs, captures Next.js's wrapper (document, router tree, layout payload), and writes the **manifest**: versioned JSON listing routes, programs, wrapper templates and fallback reasons.

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
2. Inside Next.js's request context, one **host render** (a React Flight render) runs the data phase. The route's data functions (the app's own code, unchanged) compute every hole value, in React's order. Parts that need React (hooks, uncompiled components, values the encoder doesn't support) are rendered by React in the same render.
3. Client components are looked up in the island cache by a key of component, props and the contexts they read. On a miss, React renders them.
4. The generated encoder writes all values into one reusable buffer: **one native call per flush** (ADR-0003).
5. The engine runs the program and writes HTML and Flight into reusable output buffers.
6. The runtime assembles the response from the captured wrapper and the engine's output. It streams the shell first, then each `<Suspense>` boundary when its data resolves.

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

# ADR-0003: Template programs on a prebuilt Rust engine

- **Status:** Accepted
- **Date:** 2026-10-24

## Context

- **What Phase 0 showed:** Rust rendering, fed by a generated binary encoder in one native call per route, is 7.3–7.5x React and 2.6–2.7x the best JavaScript template. The end-to-end result on a real app is 12–17x `next start`.
- **The gap:** that Rust was written per page. Users must not need a Rust toolchain, so the product ships prebuilt native binaries, and a prebuilt binary can't contain Rust generated from each user's app.

## Decision

- **Template programs:** the compiler turns each template into a **template program**: precomputed static HTML and Flight byte chunks plus a small set of ops (write escaped string, number, boolean attribute; branch; loop; client-component slot; Suspense boundary).
- **One engine:** a single prebuilt Rust engine runs every program. Data arrives through a generated encoder, in one native call per flush (shell, then each streamed boundary).
- **Versioned format:** the program format is defined in `crates/ir` and carries a version.
- **Performance gate:** before the compiler is built on top of it, the Commerce product page written as a program must reach **≥ 90% of the throughput of the hand-written Phase 0 renderer** on the same machine.

## Options considered

- **Generate Rust per app and run `cargo` at build time.** Fastest in principle, but every build machine, CI and Docker image would need a Rust toolchain and minutes of extra build time. Kept as a possible opt-in mode if the gate fails.
- **Render templates in JavaScript.** No native binary at all, but 2.6–2.7x slower than Rust in Phase 0. Rejected.

## Consequences

- Install is `npm install` only.
- The future Rust front server (Stage B) can load programs directly.
- Dev-mode template changes are near-instant (re-emit a program).
- Other targets (e.g. WebAssembly) remain possible.
- The engine must be very fast at copying static chunks and running loops and escaping. The gate checks this first.

# Testing Strategy

tsx2rs makes one promise: **output identical to Next.js, faster**. The tests are built around proving both, against Next.js and React themselves, not against our own idea of what's correct.

## Layers

| # | Layer | What it proves | Tools | Runs |
|---|---|---|---|---|
| 1 | **Unit** | One function or module behaves as specified | `cargo nextest`, Vitest | Every PR |
| 2 | **Snapshot** | Compiler phases produce the expected IR, programs, encoders and diagnostics; changes are reviewed as diffs | `insta` (Rust), Vitest snapshots | Every PR |
| 3 | **Property-based** | Invariants hold for generated inputs: escaping, encoder ↔ engine round-trip, input validation never panics | `proptest`, `fast-check` | Every PR |
| 4 | **Differential vs React** | The engine writes the same bytes as `react-dom/server` / React Flight for generated element trees | proptest + a Node oracle process | Every PR (bounded), nightly (long) |
| 5 | **Identity vs Next.js** | For every fixture app: HTML (streams resolved) and decoded Flight tree identical to `next start`; correct fallback reasons | `tests/identity` harness | Every PR |
| 6 | **Browser end-to-end** | Hydration with an empty console; interactions work; streamed boundaries reveal | Playwright | Every PR (Commerce), nightly (all) |
| 7 | **Fuzzing** | The compiler and engine never panic or hang on arbitrary input | `cargo-fuzz` (libFuzzer) | Nightly |
| 8 | **Performance** | No regressions; claims stay true | `criterion` (micro), `bench/` with oha (macro) | Every PR (micro, hot paths), before release (macro + cloud) |
| 9 | **Mutation** | The tests would catch real bugs in critical code | `cargo-mutants` on `engine` and `ir` | Weekly / before release |

## The two oracles

**React is the oracle for the engine.** Generated element trees (tags, attributes, text, nesting, Suspense states, edge-case strings with `<`, `&`, `$`, quotes, Unicode, empty text) are rendered by both `react-dom/server` and our engine, and the bytes must match.
- Any mismatch becomes a minimized, permanent regression test.
- This is how we keep up with React's rules (text separators, attribute quirks, escaping) instead of re-deriving them by hand.

**Next.js is the oracle for everything else.** The identity harness builds each fixture app with `next build`, starts `next start` and the tsx2rs runtime side by side, requests the same URLs, and compares:
- **HTML:** byte-identical after applying streamed `<Suspense>` boundaries the way the browser does, with Flight script tags removed.
- **Flight:** the decoded trees are deep-equal, with all references resolved.
- **Routing outcome:** the expected routes are served compiled, and every other route falls back with the expected reason.
Next.js's streaming chunking depends on timing, so the raw byte stream is never compared. This is ADR-0002.

## Fixtures

- Each fixture in `tests/fixtures/<area>-<feature>/` is a minimal Next.js app for one construct, e.g. `jsx-text-adjacent`, `list-map-keys`, `cond-ternary`, `clsx-dynamic`, `suspense-streamed`, `island-props-dedupe`, `metadata-og`, `fallback-unsupported-hook`.
- **Each fixture has a `README.md`** saying what it covers, and an `expect.json` listing its URLs and whether Next.js streams a boundary for them. The expected compiled/fallback status per route is added to `expect.json` when the runtime exists (milestone 1, step 8). See [`tests/fixtures/README.md`](../../tests/fixtures/README.md).
- **Every new supported construct comes with a fixture, and so does every fallback reason.**

## Rules

1. **Bug fixes start with a failing test** at the lowest layer that reproduces the bug.
2. **Tests are deterministic.** No real network, no wall-clock dependence, no shared state between tests. Randomized tests log their seed, and failures can be replayed from it.
3. **Tests are fast by default.** The PR suite targets < 10 minutes on CI; long runs go nightly.
4. **Test behaviour, not implementation.** Assert on output and public APIs, not internal calls.
5. **Snapshots are reviewed, not rubber-stamped.** An updated snapshot in a PR needs a sentence explaining why it changed.
6. **No skipped tests on `main`.** A known failure carries `#[ignore = "#issue"]` / `it.skip` with an issue link and an owner.

## Coverage

Coverage is a signal, not a goal. It finds untested code; it doesn't prove correctness. CI reports it per crate and package (`cargo llvm-cov`, Vitest v8) and fails below these floors:

| Area | Line coverage floor |
|---|---|
| `crates/ir`, `crates/engine` | 90% |
| `crates/compiler` | 85% |
| `packages/*` | 80% |

## Performance

- **Micro:** `criterion` benchmarks for the engine (static chunks, loops, escaping, Flight), the encoder, and compiler phases.
  - A PR that touches a hot path posts before/after numbers.
  - A regression above **5%** needs a justification in the PR.
- **Macro:** `bench/` reproduces the Phase 0 setup.
  - `next start` vs the tsx2rs runtime on the Commerce app.
  - Interleaved runs, two passes in opposite order, ratios on laptops.
  - Absolute numbers only from clean cloud machines.
- **Claims:** performance numbers in the README come only from a documented cloud run.

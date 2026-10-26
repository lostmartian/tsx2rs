# ADR-0006: Judge pull requests' performance by instruction counts, measured in CI

- **Status:** Accepted
- **Date:** 2026-10-26

## Context

ADR-0004 asks a pull request that touches a hot path for before/after numbers, and
docs/engineering/testing.md sets a 5% regression limit. The engine's first step-5 pull requests
(#18–#20) showed that laptop wall-clock times can't support either:

- **The same code varies too much.** Criterion runs of unchanged code differed by up to ±37%
  between runs, and a whole session ran 3 times slower than earlier ones (most likely heat).
- **Bias is easy to introduce.** A benchmark whose code a pull request didn't touch moved as much as
  the ones it did, from run order and code layout alone.
- **Workarounds were slow and still inconclusive.** Uniform code alignment, rotated runs and a
  control benchmark took about 15 minutes per pull request, sometimes twice, and still couldn't
  resolve a 5% difference.

## Decision

- **Instruction counts decide.** The engine's benchmark cases run under Valgrind's Callgrind through
  [Gungraun](https://github.com/gungraun/gungraun) (`crates/engine/benches/instructions.rs`).
  Instruction counts are deterministic: the same code gives the same count on any machine, so a
  0.1% change is real.
- **CI measures, for every pull request that touches Rust code** (the "Benchmarks" workflow): the
  base, then the pull request merged into it, with the difference per benchmark in the job
  summary. The job reports; it doesn't gate merging.
- **The 5% limit applies to instructions.** A regression above it needs a justification in the
  pull request.
- **Wall-clock numbers keep their roles:** criterion (`benches/render.rs`, the same cases) for
  absolute times and throughput, and the cloud runs (D-018 in the research repository) for every
  public performance claim.

## Options considered

- **Keep wall-clock benchmarks with stricter laptop protocols.** Tried: rotation, alignment and
  controls. Still noisier than the effects being measured. Rejected.
- **Wall-clock benchmarks on CI runners.** Shared, virtualized machines are noisier than a laptop.
  Rejected.
- **A hosted service (CodSpeed, Bencher).** Also built on instruction counts, with history and
  pull-request comments, but it adds an external service and a token for a private repository.
  Possible later; Gungraun's output is what such services consume.
- **iai-callgrind.** Gungraun's predecessor under its old name; its last release was in 2025.

## Consequences

- Instruction counts aren't times. Memory and cache effects show only partly (Callgrind estimates
  cycles), so a change that trades instructions for memory traffic still needs a wall-clock check.
- `gungraun` is pinned exactly, because `gungraun-runner` must be the same version. CI installs the
  runner version it finds in `Cargo.lock`, so a Dependabot upgrade of `gungraun` upgrades both.
- Valgrind runs only on Linux. On macOS the `instructions` benchmark is an empty program; developers
  read counts from CI.
- The encoder and compiler get instruction-count benchmarks the same way when they have hot paths.

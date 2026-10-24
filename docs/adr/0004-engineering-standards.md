# ADR-0004: Engineering standards: tooling, testing and git workflow

- **Status:** Accepted
- **Date:** 2026-10-24

## Context

The maintainer asked for gold-standard code and tests, an industry-standard layout, sound architecture and good git practice from the first commit. Choosing these before writing code avoids costly retrofits.

## Decision

- **Layout:** a Rust workspace (`crates/`) and a pnpm workspace (`packages/`) in one repository, with cross-language `tests/`, `bench/`, `docs/` and `scripts/`. This is the shape used by SWC, Rspack, Biome and oxc.
- **Rust:**
  - Pinned stable toolchain, edition 2024, `rustfmt`.
  - `clippy` (`all` + `pedantic`, `-D warnings`), `cargo deny`, `cargo nextest`.
  - No `unwrap`/`panic` in library code. `unsafe` only in `crates/napi`, documented.
- **TypeScript:**
  - Node.js 24, ESM, pnpm, and the strictest `tsconfig` options.
  - ESLint `typescript-eslint` `strictTypeChecked` + Prettier.
  - Vitest and Playwright for tests.
- **Testing:** the nine layers in `docs/engineering/testing.md`. React and Next.js are the oracles (differential and identity tests), with coverage floors and performance budgets.
- **Git:**
  - Trunk-based development, Conventional Commits, small PRs.
  - Squash merge, linear `main`, everything through pull requests, CI as the gate.
  - Lefthook for local hooks, Dependabot for updates, Changesets for releases.
- **Architecture:** the invariants in ARCHITECTURE.md (identical-or-fallback, strict layering, versioned formats, no global state, escaping in one place, no panics across FFI, deterministic builds) and ADRs for changes.

## Options considered

- **Biome instead of ESLint + Prettier:** faster and one tool, but lacks type-aware lint rules (e.g. floating promises). Revisit when it has them.
- **npm workspaces instead of pnpm:** fewer tools, but weaker isolation of dependencies and slower installs in a monorepo.
- **Merge commits or rebase merging:** squash keeps `main` one-commit-per-PR, which is easier to bisect and revert.

## Consequences

- CI must run all of this, so step 1 of milestone 1 builds it.
- Branch protection needs GitHub Pro (or a public repository) for private repos. Until then the PR-only rule is followed by agreement, and CI still gates merges by convention.

# ADR-0028: `tsx2rs start` runs the app, with a worker per core when asked

- **Status:** Accepted
- **Date:** 2026-11-08

## Context

The runtime's server (ADR-0019) was started from tests (`start()`). Running an app needs a
command, as `next start` is one, and Phase 0's measurements used one Node.js process per core (D-022):
a single process uses one core for JavaScript.

## Decision

- **`tsx2rs start [dir]`** (the `@tsx2rs/runtime` package's bin) serves an app built with
  `@tsx2rs/adapter`, with `next start`'s defaults: the current folder, port `PORT` or 3000, every
  interface. `--port`, `--host`, `--verify-every` (ADR-0027).
- **`--workers <n|auto>`** forks that many servers (`node:cluster`) sharing the port; `auto` is the
  number of cores. Each worker learns and serves on its own (its caches are its own); one that
  dies is replaced.
- The command is TypeScript run by Node.js directly (Node.js 24 strips types), as the rest of the
  runtime is.

## Consequences

- Each worker learns each URL from Next.js once: with N workers, up to N Next.js responses per URL
  before every worker serves it. Sharing what workers learn is possible later.
- Milestone 1's laptop benchmark and the Google Cloud run use this command.

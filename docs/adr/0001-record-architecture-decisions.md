# ADR-0001: Record architecture decisions

- **Status:** Accepted
- **Date:** 2026-10-24

## Context

tsx2rs makes design choices (formats, boundaries, invariants) that future contributors must understand, and must not undo by accident.

## Decision

We keep Architecture Decision Records in `docs/adr/`, numbered, one decision per file. A pull request that changes architecture, a versioned format or a public behaviour includes an ADR. Accepted ADRs are never edited, except for status changes and typo fixes; they are superseded by new ones.

## Consequences

- The reasoning behind the design lives next to the code and is reviewed like code.
- ARCHITECTURE.md describes the current state; ADRs describe how and why it got there.

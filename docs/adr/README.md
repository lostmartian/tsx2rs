# Architecture Decision Records

Decisions that shape this codebase: its boundaries, formats, invariants and tooling. Each record is short, permanent and numbered. To change a decision, write a new ADR that supersedes the old one, and mark the old one `Superseded by ADR-NNNN`.

Format: [MADR](https://adr.github.io/madr/)-style. Context → Decision → Consequences, plus the options considered.

| ADR | Title | Status |
|---|---|---|
| [0001](0001-record-architecture-decisions.md) | Record architecture decisions | Accepted |
| [0002](0002-identical-output-to-nextjs.md) | Output identical to Next.js, or fall back | Accepted |
| [0003](0003-template-programs-on-a-prebuilt-engine.md) | Template programs on a prebuilt Rust engine | Accepted |
| [0004](0004-engineering-standards.md) | Engineering standards: tooling, testing and git workflow | Accepted |
| [0005](0005-compiling-server-components.md) | Compiling server components: JavaScript computes values, templates render them | Proposed |

Background research (benchmarks, spikes, the full decision history before this repository existed) lives in the separate `tsx2rs-lab` repository.

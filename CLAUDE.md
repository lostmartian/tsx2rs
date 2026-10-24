# tsx2rs: instructions for AI agents

Follow the same rules as human contributors. They are binding:

- [CONTRIBUTING.md](CONTRIBUTING.md): git workflow (branch + PR for every change, Conventional Commits, squash merge), review checklist
- [ARCHITECTURE.md](ARCHITECTURE.md): code map and **invariants** (identical-or-fallback, layering, versioned formats, no global state, escaping only in the engine, no panics across FFI, deterministic builds)
- [docs/engineering/code-standards.md](docs/engineering/code-standards.md) and [docs/engineering/testing.md](docs/engineering/testing.md)
- [docs/adr/](docs/adr/): write an ADR for changes to architecture, formats or public behaviour

In particular:
- Never commit to `main` directly or push to it. Create a branch, commit in logical Conventional Commits, push, and open a PR with the template filled in.
- Write the failing test first for bug fixes. Every supported construct and every fallback reason gets an identity fixture.
- Run `pnpm check` (everything CI runs) before saying a change is done, and report failures honestly.
- Never copy code from the `tsx2rs-lab` spikes. Rewrite it to these standards, with tests.
- Research notes, the daily journal and the long-form decision history live in the parent `tsx2rs-lab` repository (`../doc/`). Keep them updated as its CLAUDE.md says.

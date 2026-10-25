# Contributing to tsx2rs

This document is the working agreement for everyone who changes this repository, people and AI agents alike. Read it with [ARCHITECTURE.md](ARCHITECTURE.md), [code standards](docs/engineering/code-standards.md) and [testing](docs/engineering/testing.md).

## Ground rules

1. **Correctness is the product.** Compiled output must be identical to what Next.js produces ([ADR-0002](docs/adr/0002-identical-output-to-nextjs.md)). When in doubt, fall back to Next.js; never ship different output.
2. **`main` is always releasable.** Every commit on `main` builds, passes all checks and has its docs updated.
3. **Every change goes through a pull request.** Nobody pushes to `main` directly, including maintainers and agents.
4. **Tests come with the change, not after it.** A bug fix starts with a test that fails without the fix.
5. **Decisions are written down.** A change to architecture, formats or public behaviour needs an [ADR](docs/adr/).

## Repository layout

| Path | Contents |
|---|---|
| `crates/` | Rust workspace members, one folder per crate (`crates/<name>/{Cargo.toml, src/, tests/, benches/}`) |
| `packages/` | npm workspace packages, one folder per package (`packages/<name>/{package.json, src/, test/}`) |
| `tests/` | Cross-language tests: the identity harness, fixture apps, end-to-end apps |
| `bench/` | Macro benchmarks (HTTP load) and their scripts |
| `docs/` | `adr/` (decisions), `engineering/` (standards), user docs |
| `scripts/` | Developer and CI scripts |
| `.github/` | CI workflows, templates, CODEOWNERS, Dependabot |

Crate names are prefixed (`tsx2rs_engine`); folders are not (`crates/engine`). npm packages use the `@tsx2rs/` scope.

## Setup

Requirements (pinned in the repo):
- Rust: the version in `rust-toolchain.toml` (rustup installs it on first use)
- Node.js: the version in `.node-version`
- pnpm: the version in `package.json` `packageManager`
- git 2.31 or later (for the git hooks)
- Cargo tools: `cargo install --locked cargo-nextest cargo-deny cargo-llvm-cov`

```bash
pnpm install          # JS dependencies + git hooks (lefthook)
pnpm build:native     # build the Rust addon used by the JS packages
pnpm check            # everything CI runs (scripts/check.sh)
pnpm test:identity    # just the Next.js fixture tests (builds every fixture)
cargo nextest run -p tsx2rs_differential   # just the engine-vs-React differential tests
cargo bench -p tsx2rs_engine               # engine micro-benchmarks (post before/after numbers for hot-path changes)
bash scripts/coverage.sh   # Rust coverage floors (CI runs this too)
```

## Git workflow

We use **trunk-based development**: short-lived branches off `main`, merged back by pull request, usually within a day or two.

### Branches

`<type>/<short-description>`, lowercase with hyphens. For example `feat/engine-loop-op`, `fix/flight-dollar-escape`, `docs/testing-strategy`.

Types are the same as for commits (below).

### Commits: Conventional Commits

```
<type>(<scope>): <summary in imperative mood, ≤ 72 chars, no period>

<body: what and why, wrapped at 72 columns; not how>

<footer: Refs #12, BREAKING CHANGE: …>
```

- **Types:**
  - `feat`: user-visible capability
  - `fix`: bug fix
  - `perf`: faster with the same behaviour
  - `refactor`: no behaviour change
  - `test`: tests only
  - `docs`: docs only
  - `build`: build system or dependencies
  - `ci`: CI config
  - `chore`: anything else
- **Scopes:** a crate or package name (`engine`, `compiler`, `ir`, `napi`, `adapter`, `runtime`, `native`), `identity` (the test harness in `tests/identity`), or `repo`, `deps` (`deps-dev` for development dependencies, as Dependabot writes it), `bench`.
- **One logical change per commit.** Don't mix formatting with behaviour changes. Each commit should build.
- `commit-msg` hook (commitlint) and CI check the format.

### Pull requests

- **Small.** Aim for under ~400 changed lines excluding snapshots and fixtures. Split large work into a stack of PRs.
- **Title** follows the commit format; it becomes the squash-commit message.
- **Description:** use the template. Cover what and why, how it was tested, performance impact, fallback behaviour, and docs/ADR updates.
- **Required to merge:**
  - all CI checks green
  - review approval (once there is more than one maintainer)
  - conversations resolved
- **Merge method:** **squash merge** only, which keeps `main` linear. The branch is deleted after merge.
- **Stacked PRs** (a PR whose base is another PR's branch) are merged one at a time, bottom first:
  1. Merge the bottom PR into `main` and delete its branch.
  2. Wait until the next PR's base shows `main`. The author then rebases it onto `main`, because the squash replaced the bottom PR's commits.
  3. Merge it only after its CI is green against `main`.

  Never merge a PR whose base is not `main`: it lands in the other branch, not in `main`. This happened with #5; #8 re-landed it.
- **Never** force-push `main`, rewrite published history, or merge with failing checks.

### Releases (from the first public version)

- **SemVer.** npm packages are versioned with Changesets. Rust crates share the workspace version.
- `CHANGELOG.md` follows Keep a Changelog and is generated from changesets.
- Tags: `vX.Y.Z`. Releases are built and published by CI only.

## Review checklist

Reviewers (and authors, before asking for review) check:

- [ ] Output identical to Next.js for every affected case, with an identity test or fixture proving it
- [ ] Unsupported input falls back with a diagnostic code and reason, never silently
- [ ] Tests at the right level (see [testing](docs/engineering/testing.md)); bug fixes have a regression test
- [ ] No new `unwrap`/`expect`/`panic` in library code; no `unsafe` outside `crates/napi` (and every `unsafe` has a `// SAFETY:` comment)
- [ ] Public items documented; ARCHITECTURE.md / ADR updated if boundaries or formats changed
- [ ] Performance: no regression beyond budget on the relevant benchmark (numbers in the PR for hot-path changes)
- [ ] No new dependency without a reason in the PR (license compatible with Apache-2.0, maintained, checked by `cargo deny` / audit)

## Reporting security issues

See [SECURITY.md](SECURITY.md). Don't open public issues for vulnerabilities.

## License

By contributing, you agree that your contributions are licensed under the [Apache License 2.0](LICENSE).

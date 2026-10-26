#!/usr/bin/env bash
# Runs every check CI runs, in the same order. Usage: pnpm check
set -euo pipefail
cd "$(dirname "$0")/.."

step() { printf '\n\033[1m== %s\033[0m\n' "$1"; }

step "Rust: format";        cargo fmt --all --check
step "Rust: lint";          cargo clippy --workspace --all-targets --locked -- -D warnings
step "Rust: tests";         cargo nextest run --workspace --exclude tsx2rs_differential --locked
step "Rust: doc tests";     cargo test --workspace --doc --locked
step "Rust: docs";          RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked
step "Rust: dependencies";  cargo deny --locked check
step "Native binding";      pnpm build:native
# One command per line: `set -e` ignores a failure on the left of `&&`.
step "TypeScript: types";   pnpm typecheck
step "TypeScript: root";    pnpm exec tsc -p tsconfig.json
step "TypeScript: lint";    pnpm lint
step "Format";              pnpm format:check
step "TypeScript: tests";   pnpm test
step "Identity: Next.js fixtures"; pnpm test:identity
step "Differential: engine vs React"; cargo nextest run -p tsx2rs_differential --locked

printf '\n\033[1;32mAll checks passed.\033[0m\n'

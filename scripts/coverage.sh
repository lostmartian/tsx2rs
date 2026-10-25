#!/usr/bin/env bash
# Rust line coverage, checked per crate against the floors in docs/engineering/testing.md.
# Usage: bash scripts/coverage.sh
# crates/napi is covered by the @tsx2rs/native tests in Node.js, not here; tests/differential
# is test code (and needs Node.js), so it is excluded too.
set -euo pipefail
cd "$(dirname "$0")/.."

cargo llvm-cov nextest --locked --workspace --exclude tsx2rs_napi --exclude tsx2rs_differential \
  --json --summary-only --output-path target/coverage.json
node scripts/check-coverage.ts target/coverage.json

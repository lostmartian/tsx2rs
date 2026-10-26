//! Instruction counts for the engine's hot paths, measured by Valgrind's Callgrind through
//! Gungraun: `cargo bench -p tsx2rs_engine --bench instructions` (Linux, with Valgrind and
//! `gungraun-runner` installed).
//!
//! Unlike wall-clock times, instruction counts are the same on every run, so a change of 0.1% is
//! real. CI measures each pull request against its base and reports the difference
//! (`.github/workflows/bench.yml`). Counts aren't times: they say what changed, and the
//! wall-clock benchmarks (`render.rs`) and the cloud runs (D-018) say how fast it is.

// Benchmarks may unwrap (docs/engineering/code-standards.md).
#![allow(clippy::unwrap_used, clippy::missing_panics_doc)]
// Gungraun's macros expand to fully qualified paths, which this lint flags at our call sites.
#![cfg_attr(target_os = "linux", allow(unused_qualifications))]

#[cfg(target_os = "linux")]
mod cases;

#[cfg(target_os = "linux")]
include!("instructions/linux.rs");

#[cfg(not(target_os = "linux"))]
#[expect(clippy::print_stderr, reason = "telling the developer why nothing ran")]
fn main() {
    eprintln!(
        "Instruction counts need Valgrind, which runs on Linux (see .github/workflows/bench.yml)."
    );
}

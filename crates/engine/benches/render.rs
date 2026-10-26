//! Wall-clock micro-benchmarks for the engine's hot paths (docs/engineering/testing.md,
//! performance layer), with criterion: `cargo bench -p tsx2rs_engine --bench render`.
//!
//! Laptop timings are noisy (±10% or more between runs of the same code), so PRs are compared by
//! instruction counts instead (`instructions.rs`, run by CI). Use these for absolute numbers and
//! throughput. The Commerce page itself is benchmarked against the Phase 0 renderer in the lab
//! repository (`spikes/engine-gate`, D-026): engine 123–163% of its throughput.

// Benchmarks may unwrap (docs/engineering/code-standards.md).
#![allow(clippy::unwrap_used, clippy::missing_panics_doc)]

mod cases;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use std::hint::black_box;
use tsx2rs_engine::{render, script_string};

fn bench(c: &mut Criterion) {
    let mut group = c.benchmark_group("render");
    for (name, case) in cases::all() {
        let mut out = case.output();
        group.throughput(Throughput::Bytes(
            (case.html_bytes + case.flight_bytes) as u64,
        ));
        group.bench_function(name, |b| {
            b.iter(|| {
                out.clear();
                render(black_box(&case.program), black_box(&case.input), &mut out).unwrap();
                black_box(&out);
            });
        });
    }
    group.finish();

    let flight = cases::flight_rows();
    let mut out = Vec::new();
    let mut group = c.benchmark_group("script_string");
    group.throughput(Throughput::Bytes(flight.len() as u64));
    group.bench_function("flight_row", |b| {
        b.iter(|| {
            out.clear();
            script_string(&mut out, black_box(&flight));
            black_box(&out);
        });
    });
    group.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);

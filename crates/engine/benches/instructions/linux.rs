// The Linux part of `instructions.rs` (Valgrind runs only there).

use gungraun::{library_benchmark, library_benchmark_group, main};
use std::hint::black_box;
use tsx2rs_engine::{Output, render, script_string};

/// A case with output buffers that already have room for it: the runtime reuses buffers across
/// requests, so growing them isn't counted.
fn prepared(case: cases::Case) -> (cases::Case, Output) {
    let out = case.output();
    (case, out)
}

#[library_benchmark]
#[bench::static_page(prepared(cases::static_page()))]
#[bench::text_list(prepared(cases::text_list()))]
#[bench::attributes(prepared(cases::attributes()))]
#[bench::spread_attributes(prepared(cases::spread_attributes()))]
#[bench::style(prepared(cases::style()))]
#[bench::props(prepared(cases::props()))]
fn render_case((case, mut out): (cases::Case, Output)) -> Output {
    render(black_box(&case.program), black_box(&case.input), &mut out).unwrap();
    out
}

#[library_benchmark]
#[bench::flight_rows(cases::flight_rows())]
fn script_escape(flight: String) -> Vec<u8> {
    let mut out = Vec::with_capacity(flight.len() * 2);
    script_string(&mut out, black_box(&flight));
    out
}

library_benchmark_group!(name = engine, benchmarks = [render_case, script_escape]);

main!(library_benchmark_groups = engine);

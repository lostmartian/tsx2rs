//! Micro-benchmarks for the engine's hot paths (docs/engineering/testing.md, performance layer).
//! A PR that changes the engine posts before/after numbers from `cargo bench -p tsx2rs_engine`.
//!
//! The Commerce page itself is benchmarked against the Phase 0 renderer in the lab repository
//! (`spikes/engine-gate`, D-026): engine 123–163% of its throughput.

// Benchmarks may unwrap (docs/engineering/code-standards.md).
#![allow(clippy::unwrap_used, clippy::missing_panics_doc)]

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use std::hint::black_box;
use tsx2rs_engine::{Output, render, script_string};
use tsx2rs_ir::attribute::{ElementRules, Kind};
use tsx2rs_ir::input::{InputWriter, tag};
use tsx2rs_ir::{ChunkId, Op, OpIndex, Program};

/// A program plus its input, and the output size per render (for throughput).
struct Case {
    program: Program,
    input: Vec<u8>,
    bytes: u64,
}

fn case(chunks: &[&str], ops: Vec<Op>, input: Vec<u8>) -> Case {
    let program = Program::new(chunks.iter().map(|c| c.as_bytes().into()).collect(), ops).unwrap();
    let mut out = Output::new();
    render(&program, &input, &mut out).unwrap();
    let bytes = (out.html.len() + out.flight.len()) as u64;
    Case {
        program,
        input,
        bytes,
    }
}

fn stat(html: u32, flight: u32) -> Op {
    Op::Static {
        html: ChunkId(html),
        flight: ChunkId(flight),
        text_start: false,
        text_end: false,
    }
}

/// A large fully static page: copying chunks.
fn static_page() -> Case {
    let html =
        "<div class=\"card\"><h2>Static heading</h2><p>Some static text.</p></div>".repeat(400);
    let flight = r#"["$","div",null,{"className":"card","children":"static"}],"#.repeat(400);
    case(&[&html, &flight], vec![stat(0, 1)], Vec::new())
}

/// `<ul>{items.map(i => <li>{i.name}</li>)}</ul>` with 1,000 items, some needing escaping.
fn text_list() -> Case {
    let mut w = InputWriter::new();
    w.count(1000);
    for i in 0..1000 {
        w.string(&if i % 10 == 0 {
            format!("Item <{i}> & \"more\"")
        } else {
            format!("Item number {i}")
        });
    }
    let ops = vec![
        stat(0, 1),
        Op::Loop {
            end: OpIndex(5),
            separator: Some(ChunkId(6)),
        },
        stat(2, 3),
        Op::Text,
        stat(4, 5),
        Op::EndLoop,
        stat(7, 8),
    ];
    let chunks = [
        "<ul>",
        "[",
        "<li>",
        r#"["$","li",null,{"children":"#,
        "</li>",
        "}]",
        ",",
        "</ul>",
        "]",
    ];
    case(&chunks, ops, w.finish())
}

/// 500 `<button className={…} disabled={…}>` elements.
fn attributes() -> Case {
    let mut w = InputWriter::new();
    w.count(500);
    for i in 0..500 {
        w.tag(tag::STRING).string(&format!("button button-{i}"));
        w.tag(if i % 2 == 0 { tag::TRUE } else { tag::FALSE });
    }
    let chunks = [
        "<button",                             // 0
        r#"["$","button",null,{"className":"#, // 1
        "></button>",                          // 2
        "}]",                                  // 3
        ",",                                   // 4
        "class",                               // 5
        "disabled",                            // 6
        "",                                    // 7
        r#","disabled":"#,                     // 8
    ];
    let ops = vec![
        Op::Loop {
            end: OpIndex(6),
            separator: Some(ChunkId(4)),
        },
        stat(0, 1),
        Op::Prop {
            name: ChunkId(5),
            kind: Kind::String,
        },
        stat(7, 8),
        Op::Prop {
            name: ChunkId(6),
            kind: Kind::Boolean,
        },
        stat(2, 3),
        Op::EndLoop,
    ];
    case(&chunks, ops, w.finish())
}

/// 500 `<a {...props}>` elements, each with 4 props classified at runtime.
fn spread_attributes() -> Case {
    let mut w = InputWriter::new();
    w.count(500);
    for i in 0..500 {
        w.count(4);
        w.string("className")
            .tag(tag::STRING)
            .string(&format!("link link-{i}"));
        w.string("href")
            .tag(tag::STRING)
            .string(&format!("/product/{i}"));
        w.string("aria-current")
            .tag(if i == 0 { tag::TRUE } else { tag::FALSE });
        w.string("data-index").tag(tag::NUMBER).number(f64::from(i));
    }
    let chunks = [
        "<a",                 // 0
        r#"["$","a",null,{"#, // 1
        "></a>",              // 2
        "}]",                 // 3
        ",",                  // 4
    ];
    let ops = vec![
        Op::Loop {
            end: OpIndex(4),
            separator: Some(ChunkId(4)),
        },
        stat(0, 1),
        Op::Attributes {
            rules: ElementRules::Anchor,
            comma_after: false,
        },
        stat(2, 3),
        Op::EndLoop,
    ];
    case(&chunks, ops, w.finish())
}

/// Client-component props: an array of 100 variant objects (Flight only).
fn props() -> Case {
    let mut w = InputWriter::new();
    w.tag(tag::ARRAY).count(100);
    for i in 0..100 {
        w.tag(tag::OBJECT).count(4);
        w.string("id")
            .tag(tag::STRING)
            .string(&format!("gid://shopify/ProductVariant/{i}"));
        w.string("title").tag(tag::STRING).string("Blue / M");
        w.string("availableForSale")
            .tag(if i % 3 == 0 { tag::FALSE } else { tag::TRUE });
        w.string("price").tag(tag::OBJECT).count(2);
        w.string("amount").tag(tag::STRING).string("20.0");
        w.string("currencyCode").tag(tag::STRING).string("USD");
    }
    case(&[], vec![Op::Value], w.finish())
}

fn bench(c: &mut Criterion) {
    let mut group = c.benchmark_group("render");
    for (name, case) in [
        ("static_page", static_page()),
        ("text_list", text_list()),
        ("attributes", attributes()),
        ("spread_attributes", spread_attributes()),
        ("props", props()),
    ] {
        let mut out = Output::new();
        group.throughput(Throughput::Bytes(case.bytes));
        group.bench_function(name, |b| {
            b.iter(|| {
                out.clear();
                render(black_box(&case.program), black_box(&case.input), &mut out).unwrap();
                black_box(&out);
            });
        });
    }
    group.finish();

    let row = concat!(
        r#"0:["$","div",null,{"className":"x","children":["<b>&amp;</b>",""#,
        "\u{2028}",
        r#"",{"a":"$$b"}]}]"#
    );
    let flight = row.repeat(400);
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

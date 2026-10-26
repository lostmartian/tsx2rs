//! The engine benchmarks' cases, shared by the wall-clock benchmarks (`render.rs`, criterion)
//! and the instruction counts (`instructions.rs`, Gungraun).

use tsx2rs_engine::{Output, render};
use tsx2rs_ir::attribute::{ElementRules, Kind};
use tsx2rs_ir::input::{InputWriter, tag};
use tsx2rs_ir::{ChunkId, Op, OpIndex, Program, TemplateId};

/// A program plus its input, and the size of what one render writes.
#[derive(Debug)]
pub(crate) struct Case {
    pub(crate) program: Program,
    pub(crate) input: Vec<u8>,
    pub(crate) html_bytes: usize,
    pub(crate) flight_bytes: usize,
}

impl Case {
    /// Output buffers with room for one render, as the runtime reuses them across requests.
    pub(crate) fn output(&self) -> Output {
        Output {
            html: Vec::with_capacity(self.html_bytes),
            flight: Vec::with_capacity(self.flight_bytes),
            ..Output::default()
        }
    }
}

fn case(chunks: &[&str], ops: Vec<Op>, input: Vec<u8>) -> Case {
    case_with_templates(chunks, vec![ops], input)
}

fn case_with_templates(chunks: &[&str], templates: Vec<Vec<Op>>, input: Vec<u8>) -> Case {
    let chunks = chunks.iter().map(|c| c.as_bytes().into()).collect();
    let program = Program::with_templates(chunks, templates).unwrap();
    let mut out = Output::new();
    render(&program, &input, &mut out).unwrap();
    Case {
        program,
        input,
        html_bytes: out.html.len(),
        flight_bytes: out.flight.len(),
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
pub(crate) fn static_page() -> Case {
    let html =
        "<div class=\"card\"><h2>Static heading</h2><p>Some static text.</p></div>".repeat(400);
    let flight = r#"["$","div",null,{"className":"card","children":"static"}],"#.repeat(400);
    case(&[&html, &flight], vec![stat(0, 1)], Vec::new())
}

/// `<ul>{items.map(i => <li>{i.name}</li>)}</ul>` with 1,000 items, some needing escaping.
pub(crate) fn text_list() -> Case {
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

/// `text_list`'s first 500 items, each `<li>` rendered by an `<Item>` template through
/// `Op::Call` (ADR-0008), as for components the compiler doesn't inline.
pub(crate) fn calls() -> Case {
    let mut w = InputWriter::new();
    w.count(500);
    for i in 0..500 {
        w.string(&format!("Item number {i}"));
    }
    let list = vec![
        stat(0, 1),
        Op::Loop {
            end: OpIndex(3),
            separator: Some(ChunkId(6)),
        },
        Op::Call {
            template: TemplateId(1),
        },
        Op::EndLoop,
        stat(7, 8),
    ];
    let item = vec![stat(2, 3), Op::Text, stat(4, 5)];
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
    case_with_templates(&chunks, vec![list, item], w.finish())
}

/// 500 `<button className={…} disabled={…}>` elements.
pub(crate) fn attributes() -> Case {
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
pub(crate) fn spread_attributes() -> Case {
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

/// 500 `<span style={{…}}>` elements, each with 3 style entries.
pub(crate) fn style() -> Case {
    let mut w = InputWriter::new();
    w.count(500);
    for i in 0..500 {
        w.tag(tag::OBJECT).count(3);
        w.string("backgroundColor")
            .tag(tag::STRING)
            .string("rgb(10, 20, 30)");
        w.string("marginTop")
            .tag(tag::NUMBER)
            .number(f64::from(i % 16));
        w.string("opacity").tag(tag::NUMBER).number(0.5);
    }
    let chunks = [
        "<span",                         // 0
        r#"["$","span",null,{"style":"#, // 1
        "></span>",                      // 2
        "}]",                            // 3
        ",",                             // 4
    ];
    let ops = vec![
        Op::Loop {
            end: OpIndex(4),
            separator: Some(ChunkId(4)),
        },
        stat(0, 1),
        Op::Style,
        stat(2, 3),
        Op::EndLoop,
    ];
    case(&chunks, ops, w.finish())
}

/// Client-component props: an array of 100 variant objects (Flight only).
pub(crate) fn props() -> Case {
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

/// 200 element values (ADR-0007) in one `{x}` hole: keyed
/// `<li className={…}><a href={…}>{name}</a></li>`, as a component that received them as
/// `children` would place them.
pub(crate) fn element_values() -> Case {
    let mut w = InputWriter::new();
    w.tag(tag::ARRAY).count(200);
    for i in 0..200 {
        w.tag(tag::ELEMENT).string("li");
        w.tag(tag::STRING).string(&i.to_string());
        w.count(2);
        w.string("className").tag(tag::STRING).string("menu-item");
        w.string("children");
        w.tag(tag::ELEMENT).string("a").tag(tag::NULL).count(2);
        w.string("href")
            .tag(tag::STRING)
            .string(&format!("/search/collection-{i}"));
        w.string("children");
        w.tag(tag::STRING).string(&format!("Collection {i}"));
    }
    let chunks = ["<ul>", r#"["$","ul",null,{"children":"#, "</ul>", "}]"];
    let ops = vec![stat(0, 1), Op::Child, stat(2, 3)];
    case(&chunks, ops, w.finish())
}

/// Every render case, by name.
#[allow(
    dead_code,
    reason = "the instruction counts list their cases one by one"
)]
pub(crate) fn all() -> [(&'static str, Case); 8] {
    [
        ("static_page", static_page()),
        ("text_list", text_list()),
        ("attributes", attributes()),
        ("spread_attributes", spread_attributes()),
        ("style", style()),
        ("props", props()),
        ("element_values", element_values()),
        ("calls", calls()),
    ]
}

/// A Flight payload of 400 rows to escape for an inline `<script>`.
pub(crate) fn flight_rows() -> String {
    let row = concat!(
        r#"0:["$","div",null,{"className":"x","children":["<b>&amp;</b>",""#,
        "\u{2028}",
        r#"",{"a":"$$b"}]}]"#
    );
    row.repeat(400)
}

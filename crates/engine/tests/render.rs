//! Rendering programs: each op's HTML and Flight, React's text separators, control flow, and
//! input errors.

// Tests may unwrap (docs/engineering/code-standards.md); this covers helpers outside #[test] fns.
#![allow(clippy::unwrap_used)]

use proptest::prelude::*;
use tsx2rs_engine::{MAX_VALUE_DEPTH, Output, RenderError, render};
use tsx2rs_ir::input::{InputWriter, island, tag};
use tsx2rs_ir::{ChunkId, Op, OpIndex, Program};

/// Builds programs from static strings and ops, interning chunks.
#[derive(Default)]
struct Builder {
    chunks: Vec<Box<[u8]>>,
    ops: Vec<Op>,
}

impl Builder {
    fn chunk(&mut self, s: &str) -> ChunkId {
        if let Some(i) = self.chunks.iter().position(|c| **c == *s.as_bytes()) {
            return ChunkId(u32::try_from(i).unwrap());
        }
        self.chunks.push(s.as_bytes().into());
        ChunkId(u32::try_from(self.chunks.len() - 1).unwrap())
    }

    /// Static markup (or Flight-only output): no text at its edges.
    fn stat(&mut self, html: &str, flight: &str) -> &mut Self {
        self.text(html, flight, false, false)
    }

    /// Static output whose HTML starts and/or ends with a text node. The compiler knows this
    /// from the element tree; it is never guessed from the characters.
    fn text(&mut self, html: &str, flight: &str, text_start: bool, text_end: bool) -> &mut Self {
        let (html, flight) = (self.chunk(html), self.chunk(flight));
        self.ops.push(Op::Static {
            html,
            flight,
            text_start,
            text_end,
        });
        self
    }

    fn op(&mut self, op: Op) -> &mut Self {
        self.ops.push(op);
        self
    }

    fn here(&self) -> OpIndex {
        OpIndex(u32::try_from(self.ops.len()).unwrap())
    }

    fn build(&mut self) -> Program {
        Program::new(
            std::mem::take(&mut self.chunks),
            std::mem::take(&mut self.ops),
        )
        .unwrap()
    }
}

fn input(write: impl FnOnce(&mut InputWriter)) -> Vec<u8> {
    let mut w = InputWriter::new();
    write(&mut w);
    w.finish()
}

fn run(program: &Program, input: &[u8]) -> (String, String) {
    let mut output = Output::new();
    render(program, input, &mut output).unwrap();
    (
        String::from_utf8(output.html).unwrap(),
        String::from_utf8(output.flight).unwrap(),
    )
}

fn error(program: &Program, input: &[u8]) -> RenderError {
    render(program, input, &mut Output::new()).unwrap_err()
}

#[test]
fn copies_static_chunks_to_each_output() {
    let p = Builder::default()
        .stat("<p>Hi</p>", r#"["$","p",null,{"children":"Hi"}]"#)
        .build();
    assert_eq!(
        run(&p, &[]),
        (
            "<p>Hi</p>".into(),
            r#"["$","p",null,{"children":"Hi"}]"#.into()
        )
    );
}

#[test]
fn separates_adjacent_text_like_react() {
    // <h1>Items: {3} in stock: {"2"}</h1>
    let p = Builder::default()
        .text("<h1>Items: ", "", false, true)
        .op(Op::Number)
        .text(" in stock: ", ",", true, true)
        .op(Op::Text)
        .stat("</h1>", "")
        .build();
    let (html, flight) = run(
        &p,
        &input(|w| {
            w.number(3.0).string("2");
        }),
    );
    assert_eq!(
        html,
        "<h1>Items: <!-- -->3<!-- --> in stock: <!-- -->2</h1>"
    );
    assert_eq!(flight, r#"3,"2""#);
}

#[test]
fn skips_empty_text_without_resetting_adjacency() {
    // {"a"}{""}{"b"}: React writes a<!-- -->b
    let p = Builder::default()
        .op(Op::Text)
        .op(Op::Text)
        .op(Op::Text)
        .build();
    let (html, flight) = run(
        &p,
        &input(|w| {
            w.string("a").string("").string("b");
        }),
    );
    assert_eq!(html, "a<!-- -->b");
    assert_eq!(flight, r#""a""""b""#);
}

#[test]
fn markup_between_texts_needs_no_separator() {
    let p = Builder::default()
        .op(Op::Text)
        .stat("<br/>", "")
        .op(Op::Text)
        .build();
    let (html, _) = run(
        &p,
        &input(|w| {
            w.string("a").string("b");
        }),
    );
    assert_eq!(html, "a<br/>b");
}

#[test]
fn a_flight_only_chunk_keeps_the_text_state() {
    let p = Builder::default()
        .op(Op::Text)
        .stat("", ",")
        .op(Op::Text)
        .build();
    let (html, flight) = run(
        &p,
        &input(|w| {
            w.string("a").string("b");
        }),
    );
    assert_eq!(html, "a<!-- -->b");
    assert_eq!(flight, r#""a","b""#);
}

#[test]
fn escapes_text_in_html_and_dollar_strings_in_flight() {
    let p = Builder::default().op(Op::Text).build();
    let (html, flight) = run(
        &p,
        &input(|w| {
            w.string("$5 <b>&'\"");
        }),
    );
    assert_eq!(html, "$5 &lt;b&gt;&amp;&#x27;&quot;");
    assert_eq!(flight, r#""$$5 <b>&'\"""#);
}

#[test]
fn writes_string_attributes_or_nothing() {
    let mut b = Builder::default();
    let class = b.chunk("class");
    b.stat("<li", "")
        .op(Op::Attribute { name: class })
        .stat(">", "");
    let p = b.build();
    let cases = [
        (
            input(|w| {
                w.tag(tag::STRING).string("a \"b\"");
            }),
            r#"<li class="a &quot;b&quot;">"#,
            r#""a \"b\"""#,
        ),
        (
            input(|w| {
                w.tag(tag::NULL);
            }),
            "<li>",
            "null",
        ),
        (
            input(|w| {
                w.tag(tag::UNDEFINED);
            }),
            "<li>",
            r#""$undefined""#,
        ),
    ];
    for (bytes, html, flight) in cases {
        assert_eq!(run(&p, &bytes), (html.into(), flight.into()));
    }
}

#[test]
fn writes_boolean_attributes_only_when_true() {
    let mut b = Builder::default();
    let disabled = b.chunk("disabled");
    b.stat("<input", "")
        .op(Op::BooleanAttribute { name: disabled })
        .stat("/>", "");
    let p = b.build();
    let cases = [
        (tag::TRUE, r#"<input disabled=""/>"#, "true"),
        (tag::FALSE, "<input/>", "false"),
        (tag::NULL, "<input/>", "null"),
        (tag::UNDEFINED, "<input/>", r#""$undefined""#),
    ];
    for (t, html, flight) in cases {
        assert_eq!(
            run(
                &p,
                &input(|w| {
                    w.tag(t);
                })
            ),
            (html.into(), flight.into())
        );
    }
}

#[test]
fn writes_raw_html_unescaped_and_as_a_flight_string() {
    let p = Builder::default()
        .stat("<div>", "")
        .op(Op::RawHtml)
        .stat("</div>", "")
        .build();
    let (html, flight) = run(
        &p,
        &input(|w| {
            w.string("<b>x</b>");
        }),
    );
    assert_eq!(html, "<div><b>x</b></div>");
    assert_eq!(flight, r#""<b>x</b>""#);
}

#[test]
fn writes_generic_values_to_flight_only() {
    let p = Builder::default().op(Op::Value).build();
    let bytes = input(|w| {
        w.tag(tag::OBJECT).count(4);
        w.string("$id").tag(tag::STRING).string("$x");
        w.string("n").tag(tag::NUMBER).number(-0.0);
        w.string("list")
            .tag(tag::ARRAY)
            .count(3)
            .tag(tag::TRUE)
            .tag(tag::NULL)
            .tag(tag::UNDEFINED);
        w.string("empty").tag(tag::OBJECT).count(0);
    });
    assert_eq!(
        run(&p, &bytes),
        (
            String::new(),
            r#"{"$id":"$$x","n":"$-0","list":[true,null,"$undefined"],"empty":{}}"#.into()
        )
    );
}

#[test]
fn joins_island_html_by_its_text_edges() {
    let p = Builder::default()
        .op(Op::Text)
        .op(Op::Island)
        .op(Op::Text)
        .build();
    let text_both = island::TEXT_START | island::TEXT_END;
    let (html, _) = run(
        &p,
        &input(|w| {
            w.string("a").island("b<i>c</i>d", text_both).string("e");
        }),
    );
    assert_eq!(html, "a<!-- -->b<i>c</i>d<!-- -->e");
    let (html, _) = run(
        &p,
        &input(|w| {
            w.string("a").island("<button>x</button>", 0).string("e");
        }),
    );
    assert_eq!(html, "a<button>x</button>e");
    let (html, _) = run(
        &p,
        &input(|w| {
            w.string("a").island("", text_both).string("e");
        }),
    );
    assert_eq!(
        html, "a<!-- -->e",
        "an empty island is skipped like empty text"
    );
}

fn list_program() -> Program {
    // <ul>{items.map(i => <li>{i}</li>)}</ul>, Flight children as an array
    let mut b = Builder::default();
    b.stat("<ul>", "[");
    let start = b.ops.len();
    b.op(Op::EndLoop); // placeholder for Loop
    b.stat("<li>", r#"["$","li",null,{"children":"#)
        .op(Op::Text)
        .stat("</li>", "}]");
    let end = b.here();
    b.op(Op::EndLoop);
    let comma = b.chunk(",");
    b.ops[start] = Op::Loop {
        end,
        separator: Some(comma),
    };
    b.stat("</ul>", "]");
    b.build()
}

#[test]
fn loops_over_lists_with_flight_separators() {
    let p = list_program();
    let (html, flight) = run(
        &p,
        &input(|w| {
            w.count(3).string("a").string("b").string("c");
        }),
    );
    assert_eq!(html, "<ul><li>a</li><li>b</li><li>c</li></ul>");
    assert_eq!(
        flight,
        r#"[["$","li",null,{"children":"a"}],["$","li",null,{"children":"b"}],["$","li",null,{"children":"c"}]]"#
    );
    assert_eq!(
        run(
            &p,
            &input(|w| {
                w.count(0);
            })
        ),
        ("<ul></ul>".into(), "[]".into())
    );
}

#[test]
fn nests_loops() {
    let mut b = Builder::default();
    let outer = b.ops.len();
    b.op(Op::EndLoop);
    b.op(Op::Text);
    let inner = b.ops.len();
    b.op(Op::EndLoop);
    b.op(Op::Number);
    let inner_end = b.here();
    b.op(Op::EndLoop);
    let outer_end = b.here();
    b.op(Op::EndLoop);
    b.ops[outer] = Op::Loop {
        end: outer_end,
        separator: None,
    };
    b.ops[inner] = Op::Loop {
        end: inner_end,
        separator: None,
    };
    let p = b.build();
    let (html, _) = run(
        &p,
        &input(|w| {
            w.count(2)
                .string("x")
                .count(2)
                .number(1.0)
                .number(2.0)
                .string("y")
                .count(1)
                .number(3.0);
        }),
    );
    assert_eq!(html, "x<!-- -->1<!-- -->2<!-- -->y<!-- -->3");
}

#[test]
fn branches_on_conditions() {
    // {cond ? <em>yes</em> : "no"}
    let mut b = Builder::default();
    let branch = b.ops.len();
    b.op(Op::EndLoop);
    b.stat("<em>yes</em>", r#"["$","em",null,{"children":"yes"}]"#);
    let jump = b.ops.len();
    b.op(Op::EndLoop);
    let else_ = b.here();
    b.text("no", r#""no""#, true, true);
    b.ops[branch] = Op::If { else_ };
    b.ops[jump] = Op::Jump { to: b.here() };
    let p = b.build();
    assert_eq!(
        run(&p, &[1]),
        (
            "<em>yes</em>".into(),
            r#"["$","em",null,{"children":"yes"}]"#.into()
        )
    );
    assert_eq!(run(&p, &[0]), ("no".into(), r#""no""#.into()));
}

#[test]
fn reports_bad_input_with_its_offset() {
    let text = Builder::default().op(Op::Text).build();
    assert_eq!(
        error(&text, &[5, 0, 0, 0, b'a']),
        RenderError::Truncated { offset: 4 }
    );
    assert_eq!(error(&text, &[1, 0]), RenderError::Truncated { offset: 0 });
    assert_eq!(
        error(&text, &[1, 0, 0, 0, 0xff]),
        RenderError::InvalidUtf8 { offset: 4 }
    );
    assert_eq!(
        error(&text, &[0, 0, 0, 0, 9]),
        RenderError::TrailingInput { offset: 4 }
    );

    let mut b = Builder::default();
    let name = b.chunk("x");
    let attribute = b.op(Op::Attribute { name }).build();
    assert_eq!(
        error(&attribute, &[9]),
        RenderError::BadTag { offset: 0, tag: 9 }
    );

    let mut b = Builder::default();
    let name = b.chunk("x");
    let boolean = b.op(Op::BooleanAttribute { name }).build();
    assert_eq!(
        error(&boolean, &[tag::STRING]),
        RenderError::BadTag {
            offset: 0,
            tag: tag::STRING
        }
    );

    let branch = Builder::default().op(Op::If { else_: OpIndex(1) }).build();
    assert_eq!(
        error(&branch, &[2]),
        RenderError::BadCondition {
            offset: 0,
            value: 2
        }
    );

    let value = Builder::default().op(Op::Value).build();
    assert_eq!(
        error(&value, &[42]),
        RenderError::BadTag { offset: 0, tag: 42 }
    );
}

#[test]
fn rejects_values_nested_too_deep_instead_of_overflowing_the_stack() {
    let p = Builder::default().op(Op::Value).build();
    let deep = input(|w| {
        for _ in 0..=MAX_VALUE_DEPTH {
            w.tag(tag::ARRAY).count(1);
        }
        w.tag(tag::NULL);
    });
    assert!(matches!(error(&p, &deep), RenderError::TooDeep { .. }));
    let ok = input(|w| {
        for _ in 0..MAX_VALUE_DEPTH {
            w.tag(tag::ARRAY).count(1);
        }
        w.tag(tag::NULL);
    });
    assert!(render(&p, &ok, &mut Output::new()).is_ok());
}

#[test]
fn appends_to_the_output_and_clear_empties_it() {
    let p = Builder::default().stat("x", "y").build();
    let mut output = Output::new();
    render(&p, &[], &mut output).unwrap();
    render(&p, &[], &mut output).unwrap();
    assert_eq!(
        (output.html.as_slice(), output.flight.as_slice()),
        (&b"xx"[..], &b"yy"[..])
    );
    output.clear();
    assert!(output.html.is_empty() && output.flight.is_empty());
}

proptest! {
    /// Text from the data never reaches the HTML unescaped.
    #[test]
    fn text_is_always_escaped(s in "\\PC*") {
        let p = Builder::default().stat("<p>", "").op(Op::Text).stat("</p>", "").build();
        let (html, _) = run(&p, &input(|w| { w.string(&s); }));
        let inner = &html["<p>".len()..html.len() - "</p>".len()];
        prop_assert!(!inner.contains(['<', '>', '"', '\'']));
        prop_assert!(inner.replace("&amp;", "").replace("&lt;", "").replace("&gt;", "")
            .replace("&quot;", "").replace("&#x27;", "").find('&').is_none());
    }

    /// Any input, valid or not, gives a result or an error: never a panic or a hang.
    #[test]
    fn arbitrary_input_never_panics(bytes in prop::collection::vec(any::<u8>(), 0..512)) {
        let p = list_program();
        let _ = render(&p, &bytes, &mut Output::new());
        let mut b = Builder::default();
        let name = b.chunk("a");
        let every_op = b.op(Op::Value).op(Op::Island).op(Op::RawHtml).op(Op::Number)
            .op(Op::Attribute { name }).op(Op::BooleanAttribute { name }).op(Op::If { else_: OpIndex(7) })
            .op(Op::Text).build();
        let _ = render(&every_op, &bytes, &mut Output::new());
    }
}

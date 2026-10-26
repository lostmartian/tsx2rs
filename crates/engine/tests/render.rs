//! Rendering programs: each op's HTML and Flight, React's text separators, control flow, and
//! input errors.

// Tests may unwrap (docs/engineering/code-standards.md); this covers helpers outside #[test] fns.
#![allow(clippy::unwrap_used)]

use proptest::prelude::*;
use tsx2rs_engine::{MAX_VALUE_DEPTH, Output, RenderError, render};
use tsx2rs_ir::attribute::{ElementRules, Kind};
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

/// Writes one test input.
type Write = fn(&mut InputWriter);

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

/// `<p` + one `Prop` op named `x` + `>`.
fn prop_program(kind: Kind) -> Program {
    let mut b = Builder::default();
    let name = b.chunk("x");
    b.stat("<p", "")
        .op(Op::Prop { name, kind })
        .stat(">", "")
        .build()
}

/// The HTML attribute (`<p` and `>` stripped) and the Flight value a prop renders to.
fn prop(kind: Kind, write: impl FnOnce(&mut InputWriter)) -> (String, String) {
    let (html, flight) = run(&prop_program(kind), &input(write));
    (html["<p".len()..html.len() - 1].to_owned(), flight)
}

fn string(s: &str) -> impl FnOnce(&mut InputWriter) + '_ {
    move |w| {
        w.tag(tag::STRING).string(s);
    }
}

fn number(n: f64) -> impl FnOnce(&mut InputWriter) {
    move |w| {
        w.tag(tag::NUMBER).number(n);
    }
}

fn bigint(digits: &str) -> impl FnOnce(&mut InputWriter) + '_ {
    move |w| {
        w.tag(tag::BIGINT).string(digits);
    }
}

fn just(t: u8) -> impl FnOnce(&mut InputWriter) {
    move |w| {
        w.tag(t);
    }
}

#[test]
fn props_write_nothing_for_null_and_undefined_whatever_their_kind() {
    for kind in Kind::ALL {
        assert_eq!(prop(kind, just(tag::NULL)), (String::new(), "null".into()));
        assert_eq!(
            prop(kind, just(tag::UNDEFINED)),
            (String::new(), r#""$undefined""#.into())
        );
    }
}

#[test]
fn string_props_write_every_value_but_booleans() {
    let k = Kind::String;
    assert_eq!(
        prop(k, string(r#"a "b" <c>"#)).0,
        r#" x="a &quot;b&quot; &lt;c&gt;""#
    );
    assert_eq!(prop(k, string("")).0, r#" x="""#);
    assert_eq!(
        prop(k, number(-0.0)),
        (r#" x="0""#.into(), r#""$-0""#.into())
    );
    assert_eq!(prop(k, number(1e21)).0, r#" x="1e+21""#);
    assert_eq!(prop(k, number(f64::NAN)).0, r#" x="NaN""#);
    assert_eq!(
        prop(k, bigint("-7")),
        (r#" x="-7""#.into(), r#""$n-7""#.into())
    );
    assert_eq!(prop(k, just(tag::TRUE)), (String::new(), "true".into()));
    assert_eq!(prop(k, just(tag::FALSE)), (String::new(), "false".into()));
}

#[test]
fn any_props_write_booleans_as_text() {
    assert_eq!(prop(Kind::Any, just(tag::TRUE)).0, r#" x="true""#);
    assert_eq!(prop(Kind::Any, just(tag::FALSE)).0, r#" x="false""#);
    assert_eq!(
        prop(Kind::Any, string("$v")),
        (r#" x="$v""#.into(), r#""$$v""#.into())
    );
}

#[test]
fn boolean_props_are_written_empty_when_truthy() {
    let truthy: [Write; 6] = [
        |w| {
            w.tag(tag::TRUE);
        },
        |w| {
            w.tag(tag::STRING).string("false");
        },
        |w| {
            w.tag(tag::STRING).string(" ");
        },
        |w| {
            w.tag(tag::NUMBER).number(-1.0);
        },
        |w| {
            w.tag(tag::NUMBER).number(f64::INFINITY);
        },
        |w| {
            w.tag(tag::BIGINT).string("2");
        },
    ];
    for write in truthy {
        assert_eq!(prop(Kind::Boolean, write).0, r#" x="""#);
    }
    let falsy: [Write; 5] = [
        |w| {
            w.tag(tag::FALSE);
        },
        |w| {
            w.tag(tag::STRING).string("");
        },
        |w| {
            w.tag(tag::NUMBER).number(-0.0);
        },
        |w| {
            w.tag(tag::NUMBER).number(f64::NAN);
        },
        |w| {
            w.tag(tag::BIGINT).string("0");
        },
    ];
    for write in falsy {
        assert_eq!(prop(Kind::Boolean, write).0, "");
    }
}

#[test]
fn overloaded_boolean_props_write_true_empty_and_drop_false() {
    let k = Kind::OverloadedBoolean;
    assert_eq!(prop(k, just(tag::TRUE)).0, r#" x="""#);
    assert_eq!(prop(k, just(tag::FALSE)).0, "");
    assert_eq!(prop(k, string("file.txt")).0, r#" x="file.txt""#);
    assert_eq!(prop(k, string("")).0, r#" x="""#);
    assert_eq!(prop(k, number(0.0)).0, r#" x="0""#);
}

#[test]
fn numeric_props_convert_values_like_javascript() {
    let positive = Kind::PositiveNumber;
    for (write, html) in [
        (number(1.0), r#" x="1""#),
        (number(0.5), ""),
        (number(f64::NAN), ""),
        (number(f64::INFINITY), r#" x="Infinity""#),
    ] {
        assert_eq!(prop(positive, write).0, html);
    }
    for (s, written) in [
        ("2", true),
        (" 3 ", true),
        ("0x10", true),
        ("1e0", true),
        ("0.99999999999999999", true), // rounds to 1, as in JavaScript
        ("0.5", false),
        ("", false),
        ("abc", false),
        ("-Infinity", false),
    ] {
        let expected = if written {
            format!(r#" x="{s}""#)
        } else {
            String::new()
        };
        assert_eq!(prop(positive, string(s)).0, expected, "{s:?}");
    }
    assert_eq!(prop(positive, just(tag::TRUE)).0, r#" x="true""#); // Number(true) is 1
    assert_eq!(prop(positive, just(tag::FALSE)).0, "");

    let any_number = Kind::Number;
    assert_eq!(prop(any_number, number(-3.0)).0, r#" x="-3""#);
    assert_eq!(prop(any_number, string("")).0, r#" x="""#); // Number("") is 0
    assert_eq!(prop(any_number, string("x")).0, "");
    assert_eq!(prop(any_number, just(tag::FALSE)).0, r#" x="false""#);
    assert_eq!(prop(any_number, number(f64::NAN)).0, "");
}

#[test]
fn numeric_props_reject_bigints_as_react_throws() {
    for kind in [Kind::PositiveNumber, Kind::Number] {
        assert_eq!(
            error(&prop_program(kind), &input(bigint("5"))),
            RenderError::InvalidValue { offset: 0 }
        );
    }
}

#[test]
fn url_props_block_javascript_urls_and_drop_booleans() {
    let blocked = r#" x="javascript:throw new Error(&#x27;React has blocked a javascript: URL as a security precaution.&#x27;)""#;
    for kind in [Kind::Url, Kind::NonEmptyUrl] {
        assert_eq!(prop(kind, string("/a?b=1&c")).0, r#" x="/a?b=1&amp;c""#);
        assert_eq!(
            prop(kind, string(" \tJaVa\nScRiPt:alert(1)")),
            (blocked.into(), r#"" \tJaVa\nScRiPt:alert(1)""#.into())
        );
        assert_eq!(prop(kind, just(tag::TRUE)).0, "");
        assert_eq!(prop(kind, number(7.0)).0, r#" x="7""#);
        assert_eq!(prop(kind, bigint("7")).0, r#" x="7""#);
    }
    assert_eq!(prop(Kind::Url, string("")).0, r#" x="""#);
    assert_eq!(prop(Kind::NonEmptyUrl, string("")).0, "");
}

/// Renders an `Attributes` op over the given props.
fn attributes(
    rules: ElementRules,
    comma_after: bool,
    props: &[(&str, Write)],
) -> Result<(String, String), RenderError> {
    let p = Builder::default()
        .op(Op::Attributes { rules, comma_after })
        .build();
    let bytes = input(|w| {
        w.count(props.len());
        for (name, write) in props {
            w.string(name);
            write(w);
        }
    });
    let mut output = Output::new();
    render(&p, &bytes, &mut output)?;
    Ok((
        String::from_utf8(output.html).unwrap(),
        String::from_utf8(output.flight).unwrap(),
    ))
}

#[test]
fn attribute_objects_classify_every_prop_at_runtime() {
    let props: [(&str, Write); 12] = [
        ("className", |w| {
            w.tag(tag::STRING).string("c");
        }),
        ("htmlFor", |w| {
            w.tag(tag::STRING).string("f");
        }),
        ("onClick", |w| {
            w.tag(tag::STRING).string("x()");
        }),
        ("hidden", |w| {
            w.tag(tag::TRUE);
        }),
        ("Data-Flag", |w| {
            w.tag(tag::TRUE);
        }),
        ("aria-x", |w| {
            w.tag(tag::FALSE);
        }),
        ("title", |w| {
            w.tag(tag::FALSE);
        }),
        ("ref", |w| {
            w.tag(tag::NULL);
        }),
        ("bad name", |w| {
            w.tag(tag::STRING).string("v");
        }),
        ("tabIndex", |w| {
            w.tag(tag::NUMBER).number(0.0);
        }),
        ("href", |w| {
            w.tag(tag::STRING).string("");
        }),
        ("id", |w| {
            w.tag(tag::NULL);
        }),
    ];
    let (html, flight) = attributes(ElementRules::Generic, false, &props).unwrap();
    assert_eq!(
        html,
        r#" class="c" for="f" hidden="" Data-Flag="true" aria-x="false" tabindex="0""#
    );
    assert_eq!(
        flight,
        r#""className":"c","htmlFor":"f","onClick":"x()","hidden":true,"Data-Flag":true,"aria-x":false,"title":false,"ref":null,"bad name":"v","tabIndex":0,"href":"","id":null"#
    );
    // `href=""` is written on `a` only.
    let (html, _) = attributes(ElementRules::Anchor, false, &props).unwrap();
    assert!(html.ends_with(r#" tabindex="0" href="""#), "{html}");
}

#[test]
fn attribute_objects_add_a_comma_only_after_props() {
    let one: [(&str, Write); 1] = [("id", |w| {
        w.tag(tag::STRING).string("a");
    })];
    assert_eq!(
        attributes(ElementRules::Generic, true, &one).unwrap().1,
        r#""id":"a","#
    );
    assert_eq!(
        attributes(ElementRules::Generic, true, &[]).unwrap(),
        (String::new(), String::new())
    );
}

#[test]
fn attribute_objects_reject_style_and_content_but_not_their_absence() {
    for name in ["style", "children", "dangerouslySetInnerHTML"] {
        let present: [(&str, Write); 2] = [
            ("id", |w| {
                w.tag(tag::NULL);
            }),
            (name, |w| {
                w.tag(tag::STRING).string("x");
            }),
        ];
        // The second prop's name starts after the count (4), "id" (4 + 2) and its tag (1).
        assert_eq!(
            attributes(ElementRules::Generic, false, &present),
            Err(RenderError::UnsupportedProp { offset: 11 })
        );
        let absent: [(&str, Write); 1] = [(name, |w| {
            w.tag(tag::UNDEFINED);
        })];
        assert!(attributes(ElementRules::Generic, false, &absent).is_ok());
    }
}

#[test]
fn attribute_objects_reject_refs_as_react_flight_does() {
    let props: [(&str, Write); 1] = [("ref", |w| {
        w.tag(tag::STRING).string("r");
    })];
    // The value's tag is after the count (4) and "ref" (4 + 3).
    assert_eq!(
        attributes(ElementRules::Generic, false, &props),
        Err(RenderError::InvalidValue { offset: 11 })
    );
}

#[test]
fn attribute_objects_reject_bigints_for_numeric_props() {
    let props: [(&str, Write); 1] = [("rowSpan", |w| {
        w.tag(tag::BIGINT).string("2");
    })];
    assert_eq!(
        attributes(ElementRules::Generic, false, &props),
        Err(RenderError::InvalidValue { offset: 15 })
    );
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
        w.tag(tag::OBJECT).count(5);
        w.string("$id").tag(tag::STRING).string("$x");
        w.string("n").tag(tag::NUMBER).number(-0.0);
        w.string("big").tag(tag::BIGINT).string("9007199254740993");
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
            r#"{"$id":"$$x","n":"$-0","big":"$n9007199254740993","list":[true,null,"$undefined"],"empty":{}}"#.into()
        )
    );
}

#[test]
fn renders_runtime_children_by_their_kind() {
    let p = Builder::default()
        .stat("<p>", "")
        .op(Op::Child)
        .stat("</p>", "")
        .build();
    // (input, HTML between the tags, Flight)
    let cases: [(Write, &str, &str); 9] = [
        (
            |w| {
                w.tag(tag::STRING).string("a<b");
            },
            "a&lt;b",
            r#""a<b""#,
        ),
        (
            |w| {
                w.tag(tag::STRING).string("$x");
            },
            "$x",
            r#""$$x""#,
        ),
        (
            |w| {
                w.tag(tag::STRING).string("");
            },
            "",
            r#""""#,
        ),
        (
            |w| {
                w.tag(tag::NUMBER).number(-0.0);
            },
            "0",
            r#""$-0""#,
        ),
        (
            |w| {
                w.tag(tag::BIGINT).string("-12");
            },
            "-12",
            r#""$n-12""#,
        ),
        (
            |w| {
                w.tag(tag::TRUE);
            },
            "",
            "true",
        ),
        (
            |w| {
                w.tag(tag::FALSE);
            },
            "",
            "false",
        ),
        (
            |w| {
                w.tag(tag::NULL);
            },
            "",
            "null",
        ),
        (
            |w| {
                w.tag(tag::UNDEFINED);
            },
            "",
            r#""$undefined""#,
        ),
    ];
    for (write, html, flight) in cases {
        assert_eq!(
            run(&p, &input(write)),
            (format!("<p>{html}</p>"), flight.to_owned())
        );
    }
}

#[test]
fn renders_arrays_of_children_in_order_with_text_separators() {
    let p = Builder::default()
        .text("a", r#""a""#, true, true)
        .op(Op::Child)
        .text("z", r#""z""#, true, true)
        .build();
    let bytes = input(|w| {
        w.tag(tag::ARRAY).count(6);
        w.tag(tag::STRING).string("b");
        w.tag(tag::NULL); // renders nothing: "b" and 1 are still adjacent
        w.tag(tag::NUMBER).number(1.0);
        w.tag(tag::STRING).string(""); // nothing, and keeps the text state
        w.tag(tag::ARRAY)
            .count(2)
            .tag(tag::BIGINT)
            .string("2")
            .tag(tag::FALSE);
        w.tag(tag::ARRAY).count(0);
    });
    assert_eq!(
        run(&p, &bytes),
        (
            "a<!-- -->b<!-- -->1<!-- -->2<!-- -->z".into(),
            r#""a"["b",null,1,"",["$n2",false],[]]"z""#.into()
        )
    );
}

#[test]
fn a_child_that_renders_nothing_keeps_the_text_state() {
    let p = Builder::default()
        .text("a", "", true, true)
        .op(Op::Child)
        .text("b", "", true, true)
        .build();
    let writes: [Write; 3] = [
        |w| {
            w.tag(tag::NULL);
        },
        |w| {
            w.tag(tag::STRING).string("");
        },
        |w| {
            w.tag(tag::ARRAY).count(0);
        },
    ];
    for write in writes {
        assert_eq!(run(&p, &input(write)).0, "a<!-- -->b");
    }
}

#[test]
fn rejects_child_values_react_would_not_render() {
    let p = Builder::default().op(Op::Child).build();
    assert_eq!(
        error(
            &p,
            &input(|w| {
                w.tag(tag::OBJECT).count(0);
            })
        ),
        RenderError::BadTag {
            offset: 0,
            tag: tag::OBJECT
        }
    );
    for digits in ["", "-", "-0", "01", "-01", "1.5", "1e3", "+1", "\u{661}"] {
        assert_eq!(
            error(
                &p,
                &input(|w| {
                    w.tag(tag::BIGINT).string(digits);
                })
            ),
            RenderError::BadBigInt { offset: 1 },
            "{digits:?}"
        );
    }
    let deep = input(|w| {
        for _ in 0..=MAX_VALUE_DEPTH {
            w.tag(tag::ARRAY).count(1);
        }
        w.tag(tag::NULL);
    });
    assert!(matches!(error(&p, &deep), RenderError::TooDeep { .. }));
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
    let prop = b
        .op(Op::Prop {
            name,
            kind: Kind::String,
        })
        .build();
    assert_eq!(
        error(&prop, &[9]),
        RenderError::BadTag { offset: 0, tag: 9 }
    );
    assert_eq!(
        error(&prop, &[tag::OBJECT, 0, 0, 0, 0]),
        RenderError::BadTag {
            offset: 0,
            tag: tag::OBJECT
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
        let every_op = b.op(Op::Value).op(Op::Child).op(Op::Island).op(Op::RawHtml).op(Op::Number)
            .op(Op::Prop { name, kind: Kind::PositiveNumber }).op(Op::Prop { name, kind: Kind::Url })
            .op(Op::Attributes { rules: ElementRules::Anchor, comma_after: true })
            .op(Op::If { else_: OpIndex(10) }).op(Op::Text).build();
        let _ = render(&every_op, &bytes, &mut Output::new());
    }
}

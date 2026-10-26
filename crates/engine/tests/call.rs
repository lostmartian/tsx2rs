//! Templates calling templates (ADR-0008): `Op::Call` and template nodes in `Op::Child`'s input.

// Tests may unwrap (docs/engineering/code-standards.md); this covers helpers outside #[test] fns.
#![allow(clippy::unwrap_used)]

use tsx2rs_engine::{Output, RenderError, render};
use tsx2rs_ir::input::{InputWriter, tag};
use tsx2rs_ir::{ChunkId, Op, OpIndex, Program, TemplateId};

/// Static output: `(html, flight)` chunk ids, with text-edge flags.
fn stat(html: u32, flight: u32, text_start: bool, text_end: bool) -> Op {
    Op::Static {
        html: ChunkId(html),
        flight: ChunkId(flight),
        text_start,
        text_end,
    }
}

fn call(template: u32) -> Op {
    Op::Call {
        template: TemplateId(template),
    }
}

fn program(chunks: &[&str], templates: Vec<Vec<Op>>) -> Program {
    Program::with_templates(
        chunks.iter().map(|c| c.as_bytes().into()).collect(),
        templates,
    )
    .unwrap()
}

fn run(program: &Program, input: &[u8]) -> Result<(String, String), RenderError> {
    let mut output = Output::new();
    render(program, input, &mut output)?;
    Ok((
        String::from_utf8(output.html).unwrap(),
        String::from_utf8(output.flight).unwrap(),
    ))
}

fn input(write: impl FnOnce(&mut InputWriter)) -> Vec<u8> {
    let mut w = InputWriter::new();
    write(&mut w);
    w.finish()
}

#[test]
fn a_call_runs_the_template_in_place_and_keeps_the_text_state() {
    // `<p>a{name}</p>` where the name comes from a `Label` template: `{x}!`.
    let p = program(
        &[
            "<p>a",
            r#"["$","p",null,{"children":["a","#,
            "!",
            r#","!""#,
            "</p>",
            "]}]",
        ],
        vec![
            vec![stat(0, 1, false, true), call(1), stat(4, 5, false, false)],
            vec![Op::Text, stat(2, 3, true, true)],
        ],
    );
    let (html, flight) = run(
        &p,
        &input(|w| {
            w.string("x");
        }),
    )
    .unwrap();
    assert_eq!(html, "<p>a<!-- -->x<!-- -->!</p>");
    assert_eq!(flight, r#"["$","p",null,{"children":["a","x","!"]}]"#);
}

#[test]
fn a_template_can_call_itself() {
    // A menu that renders its submenus:
    // `const Menu = ({ items }) => <ul>{items.map(i => <li>{i.title}<Menu items={i.items}/></li>)}</ul>`.
    let p = program(
        &[
            "<ul>",                            // 0
            r#"["$","ul",null,{"children":["#, // 1
            "<li>",                            // 2
            r#"["$","li",null,{"children":["#, // 3
            "",                                // 4
            ",",                               // 5
            "</li>",                           // 6
            "]}]",                             // 7
            "</ul>",                           // 8
        ],
        vec![
            vec![call(1)],
            vec![
                stat(0, 1, false, false),
                Op::Loop {
                    end: OpIndex(7),
                    separator: Some(ChunkId(5)),
                },
                stat(2, 3, false, false),
                Op::Text,
                stat(4, 5, false, false),
                call(1),
                stat(6, 7, false, false),
                Op::EndLoop,
                stat(8, 7, false, false),
            ],
        ],
    );
    // [{title: "a", items: [{title: "b", items: []}]}, {title: "c", items: []}]
    let data = input(|w| {
        w.count(2);
        w.string("a").count(1).string("b").count(0);
        w.string("c").count(0);
    });
    let (html, flight) = run(&p, &data).unwrap();
    assert_eq!(
        html,
        "<ul><li>a<ul><li>b<ul></ul></li></ul></li><li>c<ul></ul></li></ul>"
    );
    let empty = r#"["$","ul",null,{"children":[]}]"#;
    let li =
        |title: &str, menu: &str| format!(r#"["$","li",null,{{"children":["{title}",{menu}]}}]"#);
    let ul = |items: &[String]| format!(r#"["$","ul",null,{{"children":[{}]}}]"#, items.join(","));
    let expected = ul(&[li("a", &ul(&[li("b", empty)])), li("c", empty)]);
    assert_eq!(flight, expected);
}

#[test]
fn a_child_can_hold_a_compiled_component() {
    // `{item}` where `item` is a `<Badge label=…/>` element value, compiled as template 1.
    let p = program(
        &["<b>", r#"["$","b",null,{"children":"#, "</b>", "}]"],
        vec![
            vec![Op::Child],
            vec![stat(0, 1, false, false), Op::Text, stat(2, 3, false, false)],
        ],
    );
    let data = input(|w| {
        w.tag(tag::ARRAY).count(2);
        w.tag(tag::TEMPLATE).count(1).string("new");
        w.tag(tag::STRING).string("!");
    });
    assert_eq!(
        run(&p, &data).unwrap(),
        (
            "<b>new</b>!".into(),
            r#"[["$","b",null,{"children":"new"}],"!"]"#.into()
        )
    );
}

#[test]
fn rejects_template_nodes_for_missing_templates() {
    let p = program(&[], vec![vec![Op::Child]]);
    let data = input(|w| {
        w.tag(tag::TEMPLATE).count(1);
    });
    assert_eq!(
        run(&p, &data),
        Err(RenderError::BadTemplate {
            offset: 0,
            template: 1
        })
    );
}

#[test]
fn rejects_recursion_deeper_than_the_limit() {
    // A template that calls itself without reading input: validation allows it outside loops,
    // and the depth limit stops it.
    let p = program(&[], vec![vec![call(0)]]);
    assert!(matches!(run(&p, &[]), Err(RenderError::TooDeep { .. })));
}

#[test]
fn falls_back_where_react_would_turn_a_component_into_text() {
    // A textarea's value from its children: `"" + <Badge/>`.
    let p = program(
        &["<b>", "{}"],
        vec![vec![Op::Child], vec![stat(0, 1, false, false)]],
    );
    let data = input(|w| {
        w.tag(tag::ELEMENT)
            .string("textarea")
            .tag(tag::NULL)
            .count(1);
        w.string("children");
        w.tag(tag::TEMPLATE).count(1);
    });
    assert!(matches!(
        run(&p, &data),
        Err(RenderError::UnsupportedElement { .. })
    ));

    // An option's text, when the select has a value: React writes "[object Object]" for the
    // element; the engine can't skip a template's input without running it.
    let p = program(
        &["<b>", "{}"],
        vec![vec![Op::Child], vec![stat(0, 1, false, false)]],
    );
    let data = input(|w| {
        w.tag(tag::ELEMENT).string("select").tag(tag::NULL).count(2);
        w.string("value").tag(tag::STRING).string("a");
        w.string("children");
        w.tag(tag::ELEMENT).string("option").tag(tag::NULL).count(1);
        w.string("children");
        w.tag(tag::TEMPLATE).count(1);
    });
    assert!(matches!(
        run(&p, &data),
        Err(RenderError::UnsupportedElement { .. })
    ));
}

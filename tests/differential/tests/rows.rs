//! React's Flight rows (ADR-0025): for trees large enough that React defers parts of them to rows
//! of their own, the engine's output split by `outline` is React's payload, byte for byte.

// Tests may unwrap (docs/engineering/code-standards.md); this covers helpers outside #[test] fns.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::cell::RefCell;
use std::fmt::Write as _;

use proptest::prelude::*;
use proptest::test_runner::{Config, TestRunner};
use tsx2rs_differential::lower::lower;
use tsx2rs_differential::oracle::Oracle;
use tsx2rs_differential::tree::{Element, Link, Tree};
use tsx2rs_engine::{Output, outline, render};

fn element(
    tag: &'static str,
    key: Option<String>,
    props: Vec<(&str, &str)>,
    children: Vec<Tree>,
) -> Tree {
    Tree::Element(Element {
        tag,
        key,
        props: props
            .into_iter()
            .map(|(n, v)| {
                (
                    n.to_owned(),
                    tsx2rs_differential::tree::Value::String(v.to_owned()),
                )
            })
            .collect(),
        spread: false,
        runtime: false,
        children_at: None,
        children,
    })
}

/// A list of `items` keyed `<li>`s, each with a class, a text of `text` characters and, every
/// `component`th item, a component around its content (React checks there too).
fn list(items: usize, text: usize, component: usize, unicode: bool) -> Tree {
    let letter = if unicode { "é😀" } else { "ab" };
    let children = (0..items)
        .map(|i| {
            let content = element(
                "span",
                None,
                vec![("className", "label")],
                vec![Tree::Text(letter.repeat(text / 2 + i % 3))],
            );
            let content = if component > 0 && i % component == 0 {
                Tree::Component(Box::new(content), Link::Call)
            } else {
                content
            };
            element(
                "li",
                Some(format!("item-{i}")),
                vec![("className", "row $x")],
                vec![content],
            )
        })
        .collect();
    element(
        "main",
        None,
        vec![],
        vec![element("ul", None, vec![], children)],
    )
}

/// The engine's payload: its rows, as React numbers them (the page's row is 0, deferred rows in
/// task order).
fn engine_payload(tree: &Tree) -> String {
    let lowered = lower(tree).unwrap();
    let mut output = Output::new();
    render(&lowered.program, &lowered.input, &mut output).unwrap();
    let rows = outline(&output.flight, &output.marks).unwrap();
    let mut payload = String::new();
    for (id, row) in rows.iter().enumerate() {
        let mut row = String::from_utf8(row.clone()).unwrap();
        for target in (1..rows.len()).rev() {
            row = row.replace(&format!("\"$L!{target}\""), &format!("\"$L{target:x}\""));
        }
        let _ = writeln!(payload, "{id:x}:{row}");
    }
    payload
}

fn react_payload(oracle: &mut Oracle, tree: &Tree) -> String {
    unquote(&oracle.react_flight(tree).unwrap())
}

/// A JSON string literal's value.
fn unquote(literal: &str) -> String {
    let inner = &literal[1..literal.len() - 1];
    // UTF-16 units, as `\u` escapes write them (a surrogate pair is two).
    let mut units: Vec<u16> = Vec::new();
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        let decoded = if c == '\\' {
            match chars.next().unwrap() {
                'u' => {
                    let hex: String = chars.by_ref().take(4).collect();
                    units.push(u16::from_str_radix(&hex, 16).unwrap());
                    None
                }
                'n' => Some('\n'),
                't' => Some('\t'),
                'r' => Some('\r'),
                'b' => Some('\u{8}'),
                'f' => Some('\u{c}'),
                other => Some(other),
            }
        } else {
            Some(c)
        };
        if let Some(c) = decoded {
            let mut buffer = [0; 2];
            units.extend_from_slice(c.encode_utf16(&mut buffer));
        }
    }
    String::from_utf16(&units).unwrap()
}

#[test]
fn splits_rows_as_react_does() {
    let mut oracle = Oracle::start().expect("start the React oracle (node)");
    // Small, at the limit, past it, far past it (nested deferrals), with components and
    // non-ASCII text (React counts UTF-16 units).
    let mut rows_seen = Vec::new();
    for (items, text, component, unicode) in [
        (3, 10, 0, false),
        (40, 60, 0, false),
        (60, 80, 0, false),
        (60, 80, 3, false),
        (200, 120, 7, false),
        (60, 80, 2, true),
    ] {
        let tree = list(items, text, component, unicode);
        let react = react_payload(&mut oracle, &tree);
        rows_seen.push(react.lines().count());
        assert_eq!(
            engine_payload(&tree),
            react,
            "{items} items, {text} characters"
        );
    }
    // One row when small; React deferred parts of the others, nested ones included.
    assert_eq!(rows_seen[0], 1, "{rows_seen:?}");
    assert!(rows_seen[1..].iter().all(|&n| n > 1), "{rows_seen:?}");
    assert!(rows_seen[4] > 20, "{rows_seen:?}");
}

#[test]
fn splits_generated_lists_as_react_does() {
    let oracle = RefCell::new(Oracle::start().expect("start the React oracle (node)"));
    let mut runner = TestRunner::new(Config {
        cases: 48,
        ..Config::default()
    });
    runner
        .run(
            &(1usize..150, 0usize..300, 0usize..6, any::<bool>()),
            |(items, text, component, unicode)| {
                let tree = list(items, text, component, unicode);
                let react = react_payload(&mut oracle.borrow_mut(), &tree);
                prop_assert_eq!(engine_payload(&tree), react);
                Ok(())
            },
        )
        .unwrap();
}

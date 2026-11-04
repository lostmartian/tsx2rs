//! Build-time folding (ADR-0012): static nodes and props rendered for `Op::Static` chunks, by the
//! same code as at runtime.

// Tests may unwrap (docs/engineering/code-standards.md); this covers helpers outside #[test] fns.
#![allow(clippy::unwrap_used)]

use proptest::prelude::*;
use tsx2rs_engine::fold::{self, FoldError, Folded};
use tsx2rs_engine::{Output, render};
use tsx2rs_ir::attribute::ElementRules;
use tsx2rs_ir::input::{InputWriter, tag};
use tsx2rs_ir::{Op, Program};

/// A static node.
#[derive(Clone, Debug)]
enum Node {
    Text(String),
    Number(f64),
    Null,
    Bool(bool),
    Array(Vec<Node>),
    Element(&'static str, Vec<(&'static str, &'static str)>, Vec<Node>),
}

fn write(w: &mut InputWriter, node: &Node) {
    match node {
        Node::Text(s) => {
            w.tag(tag::STRING).string(s);
        }
        Node::Number(n) => {
            w.tag(tag::NUMBER).number(*n);
        }
        Node::Null => {
            w.tag(tag::NULL);
        }
        Node::Bool(b) => {
            w.tag(if *b { tag::TRUE } else { tag::FALSE });
        }
        Node::Array(nodes) => {
            w.tag(tag::ARRAY).count(nodes.len());
            for node in nodes {
                write(w, node);
            }
        }
        Node::Element(name, props, children) => {
            w.tag(tag::ELEMENT).string(name).tag(tag::NULL);
            w.count(props.len() + usize::from(!children.is_empty()));
            for (prop, value) in props {
                w.string(prop).tag(tag::STRING).string(value);
            }
            if !children.is_empty() {
                w.string("children");
                write(w, &Node::Array(children.clone()));
            }
        }
    }
}

fn input(node: &Node) -> Vec<u8> {
    let mut w = InputWriter::new();
    write(&mut w, node);
    w.finish()
}

fn folded(node: &Node) -> Folded {
    fold::node(&input(node)).unwrap()
}

/// The engine rendering `nodes` as one array (what a template's two neighbouring parts give).
fn rendered(nodes: &[Node]) -> (Vec<u8>, Vec<u8>) {
    let program = Program::new(vec![], vec![Op::Child]).unwrap();
    let mut output = Output::new();
    render(&program, &input(&Node::Array(nodes.to_vec())), &mut output).unwrap();
    (output.html, output.flight)
}

#[test]
fn folds_nodes_with_their_text_edges() {
    let text = folded(&Node::Text("a<b".into()));
    assert_eq!(
        text,
        Folded {
            html: b"a&lt;b".to_vec(),
            flight: br#""a<b""#.to_vec(),
            text_start: true,
            text_end: true,
        }
    );
    let element = folded(&Node::Element(
        "p",
        vec![("className", "c")],
        vec![Node::Text("x".into())],
    ));
    assert_eq!(element.html, br#"<p class="c">x</p>"#);
    assert_eq!(
        element.flight,
        br#"["$","p",null,{"className":"c","children":["x"]}]"#
    );
    assert!(!element.text_start && !element.text_end);
    let mixed = folded(&Node::Array(vec![
        Node::Element("b", vec![], vec![]),
        Node::Number(1.0),
    ]));
    assert!(!mixed.text_start && mixed.text_end);
    // Nothing written: no text at either edge.
    for empty in [Node::Text(String::new()), Node::Null, Node::Array(vec![])] {
        let folded = folded(&empty);
        assert!(folded.html.is_empty() && !folded.text_start && !folded.text_end);
    }
}

#[test]
fn folds_props_by_reacts_rules() {
    let mut w = InputWriter::new();
    w.count(3);
    w.string("className").tag(tag::STRING).string("a&b");
    w.string("hidden").tag(tag::TRUE);
    w.string("onClick").tag(tag::NULL);
    let props = fold::props(ElementRules::Generic, &w.finish()).unwrap();
    assert_eq!(props.html, br#" class="a&amp;b" hidden="""#);
    assert_eq!(
        props.flight,
        br#""className":"a&b","hidden":true,"onClick":null"#
    );
}

#[test]
fn refuses_what_hoists_or_hints() {
    let title = Node::Element("title", vec![], vec![Node::Text("t".into())]);
    assert_eq!(fold::node(&input(&title)), Err(FoldError::Hoists));
    let image = Node::Element("img", vec![("src", "/a.png")], vec![]);
    assert_eq!(fold::node(&input(&image)), Err(FoldError::Hoists));
    let mut w = InputWriter::new();
    w.count(1).string("children").tag(tag::STRING).string("x");
    assert!(matches!(
        fold::props(ElementRules::Generic, &w.finish()),
        Err(FoldError::Render(_))
    ));
}

fn node() -> impl Strategy<Value = Node> {
    let leaf = prop_oneof![
        3 => "[a-z<& ]{0,3}".prop_map(Node::Text),
        1 => (-5i32..5).prop_map(|n| Node::Number(f64::from(n))),
        1 => Just(Node::Null),
        1 => any::<bool>().prop_map(Node::Bool),
    ];
    leaf.prop_recursive(3, 16, 3, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 0..3).prop_map(Node::Array),
            (
                prop::sample::select(vec!["div", "b", "span"]),
                prop::collection::vec(inner, 0..3),
            )
                .prop_map(|(name, children)| Node::Element(name, vec![], children)),
        ]
    })
}

proptest! {
    /// Two folded nodes, joined with a separator where the first ends and the second starts
    /// with text, are what the engine renders for both together.
    #[test]
    fn folded_nodes_compose_like_rendered_ones(a in node(), b in node()) {
        let (fa, fb) = (folded(&a), folded(&b));
        let mut html = fa.html.clone();
        if fa.text_end && fb.text_start {
            html.extend_from_slice(b"<!-- -->");
        }
        html.extend_from_slice(&fb.html);
        let (expected_html, expected_flight) = rendered(&[a.clone(), b.clone()]);
        prop_assert_eq!(String::from_utf8(html).unwrap(), String::from_utf8(expected_html).unwrap());
        let flight = [b"[".as_slice(), &fa.flight, b",", &fb.flight, b"]"].concat();
        prop_assert_eq!(flight, expected_flight);
    }
}

#[test]
fn folds_element_edges_and_keys() {
    let div = fold::tag("div").unwrap();
    assert_eq!(div.html_open, b"<div");
    assert_eq!(div.html_close, b"</div>");
    assert!(!div.void);
    assert_eq!(div.flight_open, br#"["$","div","#);
    let br = fold::tag("br").unwrap();
    assert_eq!(br.html_close, b"/>");
    assert!(br.void);
    assert!(fold::tag("my-element").is_ok());
    for name in [
        "input", "img", "title", "select", "textarea", "1x", "a b", "",
    ] {
        assert_eq!(fold::tag(name), Err(FoldError::NotGeneric), "{name:?}");
    }
    assert_eq!(fold::key(None), b"null");
    assert_eq!(fold::key(Some("$k")), br#""$$k""#);
    assert_eq!(fold::key(Some("a\"b")), br#""a\"b""#);
    assert_eq!(fold::prop_key("$x"), br#""$x":"#);
}

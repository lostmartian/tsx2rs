//! Component elements as the collector records them: props, children, JSX text.

// Tests may unwrap (docs/engineering/code-standards.md); this covers helpers outside #[test] fns.
#![allow(clippy::unwrap_used)]

mod common;

use common::Project;
use tsx2rs_compiler::component::{PropValue, jsx_text};

#[test]
fn records_props_as_literals_expressions_and_spreads() {
    let p = Project::new(&[(
        "app/page.tsx",
        r#"const rest = {}; const x = 1;
export default () => <Box a="plain" b={'single "q"'} c={`tpl`} d={42} e={-1.5} f={true} g g2={null} h={x + 1} i={`t${x}`} {...rest} j="line
break" />;"#,
    )]);
    let graph = p.graph("app/page.tsx");
    let props: Vec<(String, PropValue)> = graph.modules[0].component_uses[0]
        .props
        .iter()
        .map(|p| (p.name.clone(), p.value.clone()))
        .collect();
    let lit = |s: &str| PropValue::Literal(s.into());
    let expr = |s: &str| PropValue::Expression(s.into());
    assert_eq!(
        props,
        vec![
            ("a".into(), lit(r#""plain""#)),
            ("b".into(), lit(r#""single \"q\"""#)),
            ("c".into(), lit(r#""tpl""#)),
            ("d".into(), lit("42")),
            ("e".into(), lit("-1.5")),
            ("f".into(), lit("true")),
            ("g".into(), lit("true")),
            ("g2".into(), lit("null")),
            ("h".into(), expr("x + 1")),
            ("i".into(), expr("`t${x}`")),
            (String::new(), PropValue::Spread("rest".into())),
            ("j".into(), lit(r#""line\nbreak""#)),
        ]
    );
}

#[test]
fn applies_jsx_whitespace_rules_to_children() {
    let p = Project::new(&[(
        "app/page.tsx",
        "export default () => <>\n  <A>\n    \n  </A>\n  <B> </B>\n  <C>{/* only a comment */}</C>\n  <D><span /></D>\n  <E>text</E>\n  <F />\n</>;",
    )]);
    let graph = p.graph("app/page.tsx");
    let children: Vec<(String, bool)> = graph.modules[0]
        .component_uses
        .iter()
        .map(|u| (u.name.clone(), u.has_children))
        .collect();
    let expected = [
        ("A", false),
        ("B", true),
        ("C", false),
        ("D", true),
        ("E", true),
        ("F", false),
    ];
    assert_eq!(
        children,
        expected
            .iter()
            .map(|(n, c)| ((*n).to_owned(), *c))
            .collect::<Vec<_>>()
    );
}

#[test]
fn trims_jsx_text_like_the_jsx_transform() {
    assert_eq!(
        jsx_text("  hello  "),
        Some("  hello  ".into()),
        "one line keeps its spaces"
    );
    assert_eq!(jsx_text("\n  a\n    b  \n"), Some("a b".into()));
    assert_eq!(jsx_text(" a \n b "), Some(" a b ".into()));
    assert_eq!(jsx_text("\n   \n\t\n"), None);
    assert_eq!(jsx_text("a\r\n  b"), Some("a b".into()));
}

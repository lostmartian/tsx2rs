//! Element nodes (ADR-0007): host elements read by `Op::Child` at runtime, written by React DOM's
//! rules for their type. Expected HTML is React DOM's own output (react-dom-server 19.3.1).

// Tests may unwrap (docs/engineering/code-standards.md); this covers helpers outside #[test] fns.
#![allow(clippy::unwrap_used)]

use tsx2rs_engine::{MAX_VALUE_DEPTH, Output, RenderError, render};
use tsx2rs_ir::input::{InputWriter, tag};
use tsx2rs_ir::{Op, Program};

/// A node or prop value, as the generated encoder writes it.
#[derive(Clone)]
enum V {
    S(&'static str),
    N(f64),
    Big(&'static str),
    B(bool),
    Null,
    Undefined,
    Arr(Vec<V>),
    Obj(Vec<(&'static str, V)>),
    El(El),
}

#[derive(Clone)]
struct El {
    tag: &'static str,
    key: Option<&'static str>,
    props: Vec<(&'static str, V)>,
    /// Where `children` is among the props, and its node.
    children: Option<(usize, Box<V>)>,
}

/// `<tag {...props}>{children}</tag>`: `children` after the props, as JSX writes it.
fn el(tag: &'static str, props: Vec<(&'static str, V)>, children: Option<V>) -> V {
    let at = props.len();
    V::El(El {
        tag,
        key: None,
        props,
        children: children.map(|c| (at, Box::new(c))),
    })
}

fn write(w: &mut InputWriter, v: &V) {
    match v {
        V::S(s) => {
            w.tag(tag::STRING).string(s);
        }
        V::N(n) => {
            w.tag(tag::NUMBER).number(*n);
        }
        V::Big(digits) => {
            w.tag(tag::BIGINT).string(digits);
        }
        V::B(b) => {
            w.tag(if *b { tag::TRUE } else { tag::FALSE });
        }
        V::Null => {
            w.tag(tag::NULL);
        }
        V::Undefined => {
            w.tag(tag::UNDEFINED);
        }
        V::Arr(items) => {
            w.tag(tag::ARRAY).count(items.len());
            for item in items {
                write(w, item);
            }
        }
        V::Obj(entries) => {
            w.tag(tag::OBJECT).count(entries.len());
            for (key, value) in entries {
                w.string(key);
                write(w, value);
            }
        }
        V::El(e) => {
            w.tag(tag::ELEMENT).string(e.tag);
            match e.key {
                Some(key) => w.tag(tag::STRING).string(key),
                None => w.tag(tag::NULL),
            };
            w.count(e.props.len() + usize::from(e.children.is_some()));
            for (i, (name, value)) in e.props.iter().enumerate() {
                if e.children.as_ref().is_some_and(|(at, _)| *at == i) {
                    w.string("children");
                }
                w.string(name);
                write(w, value);
            }
            if let Some((at, node)) = &e.children {
                if *at == e.props.len() {
                    w.string("children");
                }
                write(w, node);
            }
        }
    }
}

/// Renders one node through `Op::Child`.
fn render_node(node: &V) -> Result<(String, String), RenderError> {
    let program = Program::new(vec![], vec![Op::Child]).unwrap();
    let mut w = InputWriter::new();
    write(&mut w, node);
    let mut output = Output::new();
    render(&program, &w.finish(), &mut output)?;
    Ok((
        String::from_utf8(output.html).unwrap(),
        String::from_utf8(output.flight).unwrap(),
    ))
}

fn html(node: &V) -> String {
    render_node(node).unwrap().0
}

fn flight(node: &V) -> String {
    render_node(node).unwrap().1
}

fn invalid(node: &V) {
    assert!(
        matches!(render_node(node), Err(RenderError::InvalidValue { .. })),
        "expected InvalidValue, got {:?}",
        render_node(node)
    );
}

fn unsupported(node: &V) {
    assert!(
        matches!(
            render_node(node),
            Err(RenderError::UnsupportedElement { .. })
        ),
        "expected UnsupportedElement, got {:?}",
        render_node(node)
    );
}

#[test]
fn writes_generic_elements_and_their_flight() {
    let node = V::El(El {
        tag: "div",
        key: Some("k"),
        props: vec![("className", V::S("a")), ("hidden", V::B(true))],
        children: Some((2, Box::new(V::Arr(vec![V::S("x"), V::N(1.0)])))),
    });
    assert_eq!(
        render_node(&node).unwrap(),
        (
            r#"<div class="a" hidden="">x<!-- -->1</div>"#.into(),
            r#"["$","div","k",{"className":"a","hidden":true,"children":["x",1]}]"#.into()
        )
    );
}

#[test]
fn escapes_a_leading_dollar_in_keys_as_flight_does() {
    let node = V::El(El {
        tag: "i",
        key: Some("$k"),
        props: vec![],
        children: None,
    });
    assert_eq!(flight(&node), r#"["$","i","$$k",{}]"#);
}

#[test]
fn writes_children_in_their_place_in_flight_and_after_the_start_tag_in_html() {
    let node = V::El(El {
        tag: "p",
        key: None,
        props: vec![("id", V::S("a")), ("title", V::S("t"))],
        children: Some((1, Box::new(V::S("x")))),
    });
    assert_eq!(
        render_node(&node).unwrap(),
        (
            r#"<p id="a" title="t">x</p>"#.into(),
            r#"["$","p",null,{"id":"a","children":"x","title":"t"}]"#.into()
        )
    );
    let first = V::El(El {
        tag: "p",
        key: None,
        props: vec![("id", V::S("a"))],
        children: Some((0, Box::new(V::Null))),
    });
    assert_eq!(
        flight(&first),
        r#"["$","p",null,{"children":null,"id":"a"}]"#
    );
}

#[test]
fn nested_elements_reset_the_text_state() {
    let node = el(
        "div",
        vec![],
        Some(V::Arr(vec![
            V::S("a"),
            el("b", vec![], Some(V::S("x"))),
            V::S("c"),
        ])),
    );
    assert_eq!(html(&node), "<div>a<b>x</b>c</div>");
}

#[test]
fn void_elements_are_self_closing_and_reject_content() {
    assert_eq!(
        html(&el("br", vec![("id", V::S("x"))], None)),
        r#"<br id="x"/>"#
    );
    assert_eq!(html(&el("hr", vec![], Some(V::Null))), "<hr/>");
    assert_eq!(html(&el("hr", vec![], Some(V::Undefined))), "<hr/>");
    invalid(&el("br", vec![], Some(V::S("x"))));
    invalid(&el("hr", vec![], Some(V::B(false)))); // `null != false`
    invalid(&el(
        "wbr",
        vec![(
            "dangerouslySetInnerHTML",
            V::Obj(vec![("__html", V::S(""))]),
        )],
        None,
    ));
    invalid(&el("menuitem", vec![], Some(V::S("x"))));
    assert_eq!(html(&el("menuitem", vec![], None)), "<menuitem></menuitem>");
}

#[test]
fn rejects_invalid_tags_as_react_does() {
    for tag in ["1a", "a b", "a>", "", "é"] {
        invalid(&el(tag, vec![], None));
    }
    assert_eq!(html(&el("a1:_.-b", vec![], None)), "<a1:_.-b></a1:_.-b>");
}

#[test]
fn anchors_write_an_empty_href() {
    assert_eq!(
        html(&el("a", vec![("href", V::S(""))], None)),
        r#"<a href=""></a>"#
    );
    assert_eq!(
        html(&el("link-x", vec![("href", V::S(""))], None)),
        r#"<link-x href=""></link-x>"#
    );
    assert_eq!(
        html(&el("div", vec![("href", V::S(""))], None)),
        "<div></div>"
    );
}

#[test]
fn custom_elements_write_props_as_given() {
    let node = el(
        "my-el",
        vec![
            ("className", V::S("c")),
            ("a", V::B(true)),
            ("b", V::B(false)),
            ("c", V::N(1.0)),
            ("onclick", V::S("x")),
            ("htmlFor", V::S("f")),
        ],
        Some(V::S("x")),
    );
    assert_eq!(
        html(&node),
        r#"<my-el class="c" a="" c="1" onclick="x" htmlFor="f">x</my-el>"#
    );
    // `font-face` has a `-` but isn't a custom element.
    assert_eq!(
        html(&el(
            "font-face",
            vec![("className", V::S("c")), ("a", V::B(true))],
            None
        )),
        r#"<font-face class="c"></font-face>"#
    );
}

#[test]
fn inputs_write_name_form_props_checked_and_value_last() {
    let node = el(
        "input",
        vec![
            ("value", V::N(1.0)),
            ("className", V::S("c")),
            ("checked", V::B(false)),
            ("name", V::S("n")),
            ("type", V::S("t")),
        ],
        None,
    );
    assert_eq!(
        html(&node),
        r#"<input class="c" type="t" name="n" value="1"/>"#
    );
    let node = el(
        "input",
        vec![
            ("formTarget", V::S("t")),
            ("formAction", V::S("javascript:x")),
            ("defaultValue", V::S("d")),
            ("defaultChecked", V::B(true)),
            ("id", V::S("i")),
        ],
        None,
    );
    assert_eq!(
        html(&node),
        concat!(
            r#"<input id="i" formAction="javascript:throw new Error(&#x27;React has blocked a "#,
            r#"javascript: URL as a security precaution.&#x27;)" formTarget="t" checked="" value="d"/>"#
        )
    );
    // `checked` wins over `defaultChecked`, `value` over `defaultValue`.
    let node = el(
        "input",
        vec![
            ("checked", V::B(false)),
            ("defaultChecked", V::B(true)),
            ("value", V::S("v")),
            ("defaultValue", V::S("d")),
        ],
        None,
    );
    assert_eq!(html(&node), r#"<input value="v"/>"#);
    invalid(&el("input", vec![], Some(V::S("x"))));
}

#[test]
fn buttons_and_forms_write_their_form_props_last() {
    let button = el(
        "button",
        vec![
            ("formAction", V::S("/a")),
            ("name", V::S("n")),
            ("id", V::S("i")),
        ],
        Some(V::S("b")),
    );
    assert_eq!(
        html(&button),
        r#"<button id="i" name="n" formAction="/a">b</button>"#
    );
    let form = el(
        "form",
        vec![
            ("target", V::S("t")),
            ("action", V::S("/a")),
            ("id", V::S("i")),
            ("method", V::S("post")),
        ],
        None,
    );
    assert_eq!(
        html(&form),
        r#"<form id="i" action="/a" method="post" target="t"></form>"#
    );
}

#[test]
fn options_are_selected_by_the_enclosing_select() {
    let node = el(
        "select",
        vec![("value", V::S("b"))],
        Some(V::Arr(vec![
            el("option", vec![("value", V::S("a"))], Some(V::S("A"))),
            el("div", vec![], Some(el("option", vec![], Some(V::S("b"))))),
        ])),
    );
    assert_eq!(
        render_node(&node).unwrap().0,
        r#"<select><option value="a">A</option><div><option selected="">b</option></div></select>"#
    );
    let multiple = el(
        "select",
        vec![
            ("value", V::Arr(vec![V::S("a"), V::N(1.0)])),
            ("multiple", V::B(true)),
        ],
        Some(V::Arr(vec![
            el("option", vec![("value", V::S("a"))], None),
            el("option", vec![], Some(V::N(1.0))),
            el("option", vec![], Some(V::S("2"))),
        ])),
    );
    assert_eq!(
        html(&multiple),
        concat!(
            r#"<select multiple=""><option value="a" selected=""></option>"#,
            r#"<option selected="">1</option><option>2</option></select>"#
        )
    );
    assert_eq!(
        flight(&multiple),
        concat!(
            r#"["$","select",null,{"value":["a",1],"multiple":true,"children":["#,
            r#"["$","option",null,{"value":"a"}],["$","option",null,{"children":1}],"#,
            r#"["$","option",null,{"children":"2"}]]}]"#
        )
    );
}

#[test]
fn a_select_falls_back_to_its_default_value() {
    let node = el(
        "select",
        vec![("value", V::Null), ("defaultValue", V::S("x"))],
        Some(el("option", vec![], Some(V::S("x")))),
    );
    assert_eq!(
        html(&node),
        r#"<select><option selected="">x</option></select>"#
    );
}

#[test]
fn option_text_joins_strings_numbers_and_bigints() {
    let option = el(
        "option",
        vec![],
        Some(V::Arr(vec![
            V::N(1.0),
            V::S("a"),
            V::Arr(vec![V::Big("2"), V::B(true), V::Null]),
        ])),
    );
    let select = |value| el("select", vec![("value", V::S(value))], Some(option.clone()));
    assert!(html(&select("1a2")).contains("selected"));
    let with_element = el(
        "option",
        vec![],
        Some(V::Arr(vec![V::S("a"), el("b", vec![], None)])),
    );
    let node = el(
        "select",
        vec![("value", V::S("a[object Object]"))],
        Some(with_element),
    );
    assert!(html(&node).contains("selected"));
}

#[test]
fn new_contexts_reset_the_select_value() {
    for wrapper in [
        "svg",
        "math",
        "table",
        "tr",
        "noscript",
        "picture",
        "foreignObject",
    ] {
        let node = el(
            "select",
            vec![("value", V::S("a"))],
            Some(el(
                wrapper,
                vec![],
                Some(el("option", vec![], Some(V::S("a")))),
            )),
        );
        assert!(!html(&node).contains("selected"), "{wrapper}");
    }
}

#[test]
fn options_outside_a_select_use_their_selected_prop() {
    assert_eq!(
        html(&el("option", vec![("selected", V::S("x"))], None)),
        r#"<option selected=""></option>"#
    );
    assert_eq!(
        html(&el("option", vec![("selected", V::N(0.0))], None)),
        "<option></option>"
    );
}

#[test]
fn textareas_write_their_value_as_content() {
    assert_eq!(
        html(&el("textarea", vec![("value", V::S("\nx"))], None)),
        "<textarea>\n\nx</textarea>"
    );
    assert_eq!(
        html(&el("textarea", vec![("defaultValue", V::S("<"))], None)),
        "<textarea>&lt;</textarea>"
    );
    assert_eq!(
        html(&el("textarea", vec![], Some(V::Arr(vec![V::S("a")])))),
        "<textarea>a</textarea>"
    );
    assert_eq!(
        html(&el(
            "textarea",
            vec![],
            Some(V::Arr(vec![V::Arr(vec![V::N(1.0), V::N(2.0)])]))
        )),
        "<textarea>1,2</textarea>"
    );
    // The children are the value; React doesn't render them, and Flight still has them.
    let node = el("textarea", vec![], Some(el("title", vec![], None)));
    assert_eq!(
        render_node(&node).unwrap(),
        (
            "<textarea>[object Object]</textarea>".into(),
            r#"["$","textarea",null,{"children":["$","title",null,{}]}]"#.into()
        )
    );
    invalid(&el(
        "textarea",
        vec![],
        Some(V::Arr(vec![V::S("a"), V::S("b")])),
    ));
    invalid(&el("textarea", vec![("value", V::S("v"))], Some(V::S("c"))));
    invalid(&el(
        "textarea",
        vec![(
            "dangerouslySetInnerHTML",
            V::Obj(vec![("__html", V::S(""))]),
        )],
        None,
    ));
}

#[test]
fn pre_and_listing_double_a_leading_newline() {
    assert_eq!(
        html(&el("pre", vec![], Some(V::S("\nx")))),
        "<pre>\n\nx</pre>"
    );
    assert_eq!(
        html(&el("listing", vec![], Some(V::S("\nx")))),
        "<listing>\n\nx</listing>"
    );
    assert_eq!(
        html(&el("pre", vec![], Some(V::Arr(vec![V::S("\nx")])))),
        "<pre>\nx</pre>"
    );
    let inner = V::Obj(vec![("__html", V::S("\n<b>"))]);
    assert_eq!(
        html(&el("pre", vec![("dangerouslySetInnerHTML", inner)], None)),
        "<pre>\n\n<b></pre>"
    );
}

#[test]
fn objects_write_data_as_a_url() {
    assert_eq!(
        html(&el("object", vec![("data", V::S(""))], None)),
        "<object></object>"
    );
    assert_eq!(
        html(&el("object", vec![("data", V::B(true))], None)),
        r#"<object data="true"></object>"#
    );
    assert!(
        html(&el("object", vec![("data", V::S("javascript:x"))], None))
            .contains("React has blocked")
    );
}

#[test]
fn writes_inner_html_unescaped_and_checks_it_like_react() {
    let inner = |v| ("dangerouslySetInnerHTML", V::Obj(vec![("__html", v)]));
    assert_eq!(
        html(&el("div", vec![inner(V::S("<b>&</b>"))], None)),
        "<div><b>&</b></div>"
    );
    assert_eq!(
        html(&el("div", vec![inner(V::N(1.5))], None)),
        "<div>1.5</div>"
    );
    assert_eq!(
        html(&el("div", vec![inner(V::Null)], Some(V::Null))),
        "<div></div>"
    );
    assert_eq!(
        flight(&el("div", vec![inner(V::S("x"))], None)),
        r#"["$","div",null,{"dangerouslySetInnerHTML":{"__html":"x"}}]"#
    );
    invalid(&el("div", vec![inner(V::S("x"))], Some(V::S("y"))));
    invalid(&el(
        "div",
        vec![("dangerouslySetInnerHTML", V::Obj(vec![]))],
        None,
    ));
    invalid(&el(
        "div",
        vec![("dangerouslySetInnerHTML", V::S("x"))],
        None,
    ));
    invalid(&el(
        "div",
        vec![("dangerouslySetInnerHTML", V::Arr(vec![]))],
        None,
    ));
    assert_eq!(
        html(&el("div", vec![("dangerouslySetInnerHTML", V::Null)], None)),
        "<div></div>"
    );
}

#[test]
fn rejects_elements_react_hoists_or_the_document_provides() {
    for tag in [
        "title", "meta", "link", "script", "style", "html", "head", "body",
    ] {
        unsupported(&el(tag, vec![], None));
    }
}

#[test]
fn rejects_images_react_would_preload() {
    unsupported(&el("img", vec![("src", V::S("/x.png"))], None));
    unsupported(&el("img", vec![("srcSet", V::S("/x.png 2x"))], None));
    let written = [
        vec![("src", V::S("/x.png")), ("loading", V::S("lazy"))],
        vec![("src", V::S("/x.png")), ("fetchPriority", V::S("low"))],
        vec![("src", V::S(""))],
        vec![("src", V::S("data:x"))],
        vec![("src", V::S("DaTa:x"))],
        vec![("src", V::N(1.0))],
        vec![("alt", V::S("a"))],
    ];
    for props in written {
        assert!(html(&el("img", props, None)).starts_with("<img"));
    }
    for scope in ["noscript", "picture"] {
        let node = el(
            scope,
            vec![],
            Some(el("img", vec![("src", V::S("/x.png"))], None)),
        );
        assert_eq!(
            html(&node),
            format!(r#"<{scope}><img src="/x.png"/></{scope}>"#)
        );
    }
}

#[test]
fn rejects_refs_and_a_second_children_entry() {
    invalid(&el("div", vec![("ref", V::S("r"))], None));
    assert_eq!(
        html(&el("div", vec![("ref", V::Null)], None)),
        "<div></div>"
    );
    let mut w = InputWriter::new();
    w.tag(tag::ELEMENT).string("div").tag(tag::NULL).count(2);
    w.string("children").string("children").tag(tag::NULL);
    let program = Program::new(vec![], vec![Op::Child]).unwrap();
    assert!(matches!(
        render(&program, &w.finish(), &mut Output::new()),
        Err(RenderError::UnsupportedProp { .. })
    ));
}

#[test]
fn rejects_elements_nested_too_deep() {
    let mut node = el("i", vec![], None);
    for _ in 0..=MAX_VALUE_DEPTH {
        node = el("i", vec![], Some(node));
    }
    assert!(matches!(
        render_node(&node),
        Err(RenderError::TooDeep { .. })
    ));
}

#[test]
fn rejects_malformed_element_input() {
    // A key that is neither null nor a string.
    let mut w = InputWriter::new();
    w.tag(tag::ELEMENT)
        .string("div")
        .tag(tag::NUMBER)
        .number(1.0)
        .count(0);
    let program = Program::new(vec![], vec![Op::Child]).unwrap();
    assert!(matches!(
        render(&program, &w.finish(), &mut Output::new()),
        Err(RenderError::BadTag { .. })
    ));
    // An object attribute value (the encoder sends those to React).
    assert!(matches!(
        render_node(&el("div", vec![("id", V::Obj(vec![]))], None)),
        Err(RenderError::BadTag { .. })
    ));
}

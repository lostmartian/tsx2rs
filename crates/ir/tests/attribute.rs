//! React DOM's prop rules: how each prop name is classified.

use tsx2rs_ir::attribute::{ElementRules, Kind, Prop, classify, is_safe_name};

fn attribute(prop: &str) -> Option<(&str, Kind)> {
    match classify(prop, ElementRules::Generic) {
        Prop::Attribute { name, kind } => Some((name, kind)),
        _ => None,
    }
}

#[test]
fn renames_and_classifies_the_props_react_knows() {
    let cases = [
        ("className", "class", Kind::String),
        ("tabIndex", "tabindex", Kind::String),
        ("viewBox", "viewBox", Kind::String),
        ("htmlFor", "for", Kind::String),
        ("httpEquiv", "http-equiv", Kind::String),
        ("strokeWidth", "stroke-width", Kind::String),
        ("xmlnsXlink", "xmlns:xlink", Kind::String),
        ("xlinkTitle", "xlink:title", Kind::String),
        ("xmlLang", "xml:lang", Kind::String),
        ("value", "value", Kind::Any),
        ("draggable", "draggable", Kind::Any),
        ("hidden", "hidden", Kind::Boolean),
        ("itemScope", "itemScope", Kind::Boolean),
        ("autoFocus", "autofocus", Kind::Boolean),
        ("download", "download", Kind::OverloadedBoolean),
        ("cols", "cols", Kind::PositiveNumber),
        ("rowSpan", "rowSpan", Kind::Number),
        ("src", "src", Kind::NonEmptyUrl),
        ("href", "href", Kind::NonEmptyUrl),
        ("formAction", "formAction", Kind::Url),
        ("xlinkHref", "xlink:href", Kind::Url),
    ];
    for (prop, name, kind) in cases {
        assert_eq!(attribute(prop), Some((name, kind)), "{prop}");
    }
    assert_eq!(
        classify("href", ElementRules::Anchor),
        Prop::Attribute {
            name: "href",
            kind: Kind::Url
        }
    );
}

#[test]
fn passes_unknown_names_through_and_writes_booleans_only_for_data_and_aria() {
    assert_eq!(attribute("data-id"), Some(("data-id", Kind::Any)));
    assert_eq!(attribute("ARIA-label"), Some(("ARIA-label", Kind::Any)));
    assert_eq!(attribute("data"), Some(("data", Kind::String)));
    assert_eq!(attribute("datax"), Some(("datax", Kind::String)));
    assert_eq!(attribute("my-attr"), Some(("my-attr", Kind::String)));
    assert_eq!(attribute("on"), Some(("on", Kind::String))); // not an event handler
    assert_eq!(attribute("é:x.y"), Some(("é:x.y", Kind::String)));
}

#[test]
fn ignores_handlers_internal_props_and_unsafe_names() {
    for prop in [
        "onClick",
        "ONCLICK",
        "onx",
        "defaultValue",
        "defaultChecked",
        "innerHTML",
        "suppressHydrationWarning",
        "suppressContentEditableWarning",
        "",
        "1x",
        "-x",
        "a b",
        "a\"b",
        "a=b",
        "a>b",
        "x/",
        "😀",
        "a😀",
    ] {
        assert_eq!(
            classify(prop, ElementRules::Generic),
            Prop::Ignored,
            "{prop:?}"
        );
    }
    assert_eq!(classify("style", ElementRules::Generic), Prop::Style);
    assert_eq!(classify("ref", ElementRules::Generic), Prop::Ref);
    assert_eq!(classify("children", ElementRules::Anchor), Prop::Content);
    assert_eq!(
        classify("dangerouslySetInnerHTML", ElementRules::Generic),
        Prop::Content
    );
}

#[test]
fn checks_names_like_reacts_regex() {
    for safe in [
        ":", "_", "a", "Z", "x-1.2", "x\u{B7}", "\u{C0}", "\u{FFFD}", "a\u{300}",
    ] {
        assert!(is_safe_name(safe), "{safe:?}");
    }
    for unsafe_name in [
        "",
        "1",
        ".",
        "-",
        "\u{D7}",
        "\u{F7}",
        "\u{B7}",
        "\u{FFFE}",
        "a\u{2041}",
    ] {
        assert!(!is_safe_name(unsafe_name), "{unsafe_name:?}");
    }
}

//! Generated React trees: the model, its JSON form for the oracle, and the generator.

use std::fmt::Write as _;

use proptest::prelude::*;

/// A React node: what a server component's template can produce for host elements.
#[derive(Clone, Debug)]
pub enum Tree {
    /// A string child (`""` included: React renders nothing for it).
    Text(String),
    /// A number child.
    Number(f64),
    /// `null`
    Null,
    /// `true` / `false` (React renders nothing for booleans).
    Bool(bool),
    /// `undefined`
    Undefined,
    /// A bigint child: its decimal digits.
    BigInt(String),
    /// An array of nodes (React renders them in order).
    Array(Vec<Self>),
    /// A node known only at runtime: lowered to `Op::Child`, with the node as its input. Holds
    /// primitives and arrays (an element value is an [`Element`] with `runtime` set).
    Hole(Box<Self>),
    /// A host element.
    Element(Element),
}

/// A host element (`<div>`, `<a>`, `<br>`, `<input>`, `<my-element>`, …).
#[derive(Clone, Debug)]
pub struct Element {
    /// The tag.
    pub tag: &'static str,
    /// The key, if any.
    pub key: Option<String>,
    /// Props in order (unique names): React prop name and value. A `style` prop's value is a
    /// [`Value::Style`], `null` or `undefined`.
    pub props: Vec<(String, Value)>,
    /// Whether the props come from a spread (`<div {...props}>`): the compiler doesn't know
    /// their names, so they're lowered to one `Op::Attributes`. Otherwise each prop is an
    /// `Op::Prop` (or `Op::Value` for props React doesn't write).
    pub spread: bool,
    /// Whether the element is an element value (ADR-0007): lowered to `Op::Child` with the
    /// whole element, its descendants included, as input. Always set for [`SPECIAL_TAGS`].
    pub runtime: bool,
    /// Where `children` is among the props, as `<div {...{ children, id }}>` puts it. `None`:
    /// after the props, as JSX writes it (and absent when there are no children). Only for
    /// element values.
    pub children_at: Option<usize>,
    /// Children.
    pub children: Vec<Tree>,
}

impl Element {
    /// Whether the tag is a void element (no content, written `<tag/>`).
    #[must_use]
    pub fn is_void(&self) -> bool {
        VOID_TAGS.contains(&self.tag) || matches!(self.tag, "input" | "img" | "source")
    }

    /// The `children` prop's value, as `createElement(tag, props, ...children)` builds it: absent
    /// for none (unless `children_at` places it), the child for one, an array for several.
    #[must_use]
    pub fn children_value(&self) -> Option<Tree> {
        match self.children.as_slice() {
            [] if self.children_at.is_none() => None,
            [only] => Some(only.clone()),
            children => Some(Tree::Array(children.to_vec())),
        }
    }
}

/// A prop's value: a primitive.
#[derive(Clone, Debug)]
pub enum Value {
    /// A string.
    String(String),
    /// A number.
    Number(f64),
    /// A bigint: its decimal digits.
    BigInt(String),
    /// A boolean.
    Bool(bool),
    /// `null`
    Null,
    /// `undefined`
    Undefined,
    /// A `style` object: CSS property names and primitive values, in order (unique names).
    Style(Vec<(String, Value)>),
    /// An array of primitives: a `<select>`'s `value` or `defaultValue` (`multiple`).
    Array(Vec<Value>),
    /// `{ __html: value }` (`dangerouslySetInnerHTML`).
    Html(Box<Value>),
}

/// Tags whose props follow React DOM's generic rules, plus `a` (the only difference: `href=""`
/// is written) and custom elements (their own prop rules, `ElementRules::Custom`). These can be
/// lowered to templates or be element values.
pub const TAGS: &[&str] = &[
    "div",
    "span",
    "p",
    "ul",
    "li",
    "b",
    "i",
    "section",
    "h1",
    "a",
    "svg",
    "g",
    "path",
    "td",
    "my-element",
    "x-y",
];
/// Void tags with generic prop rules (React's `pushSelfClosing`). `input`, `img` and `source` are
/// void too, in [`SPECIAL_TAGS`].
pub const VOID_TAGS: &[&str] = &["br", "hr", "wbr"];
/// Tags with rules of their own (ADR-0007), or that change their descendants' context. The
/// compiler lowers these as element values, so they're always generated as element values.
pub const SPECIAL_TAGS: &[&str] = &[
    "input",
    "img",
    "source",
    "textarea",
    "select",
    "option",
    "optgroup",
    "button",
    "form",
    "pre",
    "listing",
    "object",
    "menuitem",
    "noscript",
    "picture",
    "table",
    "tr",
    "math",
    "font-face",
];

/// Prop names covering every class of React DOM's `pushAttribute`: renamed, string, any,
/// boolean, overloaded boolean, numeric and URL props, ignored props, and aliases. Names React
/// treats specially elsewhere are left out: `style` (its own op), `children` and
/// `dangerouslySetInnerHTML` (content), `key` (not a prop) and `ref` (React's Flight server
/// throws on it).
const KNOWN_PROPS: &[&str] = &[
    "className",
    "tabIndex",
    "dir",
    "role",
    "viewBox",
    "width",
    "height",
    "id",
    "title",
    "src",
    "href",
    "action",
    "formAction",
    "xlinkHref",
    "defaultValue",
    "defaultChecked",
    "innerHTML",
    "suppressHydrationWarning",
    "suppressContentEditableWarning",
    "autoFocus",
    "multiple",
    "muted",
    "contentEditable",
    "spellCheck",
    "draggable",
    "value",
    "focusable",
    "hidden",
    "disabled",
    "inert",
    "itemScope",
    "open",
    "capture",
    "download",
    "cols",
    "rows",
    "size",
    "span",
    "rowSpan",
    "start",
    "xlinkTitle",
    "xmlLang",
    "htmlFor",
    "httpEquiv",
    "strokeWidth",
    "xmlnsXlink",
    "panose-1",
    "onClick",
    "onmouseover",
    "on",
    "data-id",
    "Data-Flag",
    "aria-hidden",
    "ARIA-x",
    "data",
    "my-attr",
    "x:y",
    "é",
    "name",
    "type",
    "checked",
    "selected",
];

/// Props that elements in [`SPECIAL_TAGS`] treat specially, by tag.
fn special_props(tag: &str) -> &'static [&'static str] {
    const FORM_ACTION: &[&str] = &[
        "name",
        "formAction",
        "formEncType",
        "formMethod",
        "formTarget",
        "type",
        "id",
        "dangerouslySetInnerHTML",
    ];
    match tag {
        "input" => &[
            "name",
            "formAction",
            "formEncType",
            "formMethod",
            "formTarget",
            "value",
            "defaultValue",
            "checked",
            "defaultChecked",
            "type",
            "id",
        ],
        "button" => FORM_ACTION,
        "form" => &["action", "encType", "method", "target", "id"],
        "select" => &["value", "defaultValue", "multiple", "id"],
        "option" => &["value", "selected", "label", "dangerouslySetInnerHTML"],
        "textarea" => &["value", "defaultValue", "rows", "id"],
        "img" | "source" => &["src", "srcSet", "loading", "fetchPriority", "alt"],
        "object" => &["data", "type"],
        _ => &["dangerouslySetInnerHTML", "id"],
    }
}

/// Names React drops as unsafe in HTML.
const UNSAFE_PROPS: &[&str] = &["1x", "-x", "a b", "a\"b", "a=b", "x>", "😀"];

/// Props React converts to a number (`isNaN` throws on a bigint, so they get no bigints).
const NUMERIC_PROPS: &[&str] = &["cols", "rows", "size", "span", "rowSpan", "start"];

// ---------------------------------------------------------------- JSON for the oracle

impl Tree {
    /// The tree as the oracle reads it (`oracle/oracle.ts`, type `Tree`).
    #[must_use]
    pub fn to_json(&self) -> String {
        let mut out = String::new();
        self.write_json(&mut out);
        out
    }

    fn write_json(&self, out: &mut String) {
        match self {
            Self::Text(s) => json_string(out, s),
            Self::Number(n) => number_json(out, *n),
            Self::Null => out.push_str("null"),
            Self::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Self::Undefined => out.push_str(r#"{"u":1}"#),
            Self::BigInt(digits) => bigint_json(out, digits),
            Self::Array(nodes) => {
                out.push('[');
                for (i, node) in nodes.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    node.write_json(out);
                }
                out.push(']');
            }
            Self::Hole(node) => node.write_json(out), // React sees the value itself
            Self::Element(e) => {
                out.push_str(r#"{"t":"#);
                json_string(out, e.tag);
                if let Some(key) = &e.key {
                    out.push_str(r#","k":"#);
                    json_string(out, key);
                }
                if let Some(at) = e.children_at {
                    let _ = write!(out, r#","ci":{at}"#);
                }
                out.push_str(r#","a":["#);
                for (i, (name, value)) in e.props.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    out.push('[');
                    json_string(out, name);
                    out.push(',');
                    value_json(out, value);
                    out.push(']');
                }
                out.push_str(r#"],"c":["#);
                for (i, child) in e.children.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    child.write_json(out);
                }
                out.push_str("]}");
            }
        }
    }
}

fn value_json(out: &mut String, value: &Value) {
    match value {
        Value::String(s) => json_string(out, s),
        Value::Number(n) => number_json(out, *n),
        Value::BigInt(digits) => bigint_json(out, digits),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Null => out.push_str("null"),
        Value::Undefined => out.push_str(r#"{"u":1}"#),
        Value::Array(items) => {
            out.push_str(r#"{"a":["#);
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                value_json(out, item);
            }
            out.push_str("]}");
        }
        Value::Html(inner) => {
            out.push_str(r#"{"h":"#);
            value_json(out, inner);
            out.push('}');
        }
        Value::Style(entries) => {
            out.push_str(r#"{"s":["#);
            for (i, (name, value)) in entries.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push('[');
                json_string(out, name);
                out.push(',');
                value_json(out, value);
                out.push(']');
            }
            out.push_str("]}");
        }
    }
}

fn number_json(out: &mut String, n: f64) {
    out.push_str(r#"{"n":"#);
    json_string(out, &number_repr(n));
    out.push('}');
}

fn bigint_json(out: &mut String, digits: &str) {
    out.push_str(r#"{"b":"#);
    json_string(out, digits);
    out.push('}');
}

/// A number as text JavaScript's `Number()` parses back exactly (including `NaN`, `-0`,
/// `Infinity`).
fn number_repr(n: f64) -> String {
    if n.is_nan() {
        "NaN".into()
    } else if n.is_infinite() {
        (if n > 0.0 { "Infinity" } else { "-Infinity" }).into()
    } else if n == 0.0 && n.is_sign_negative() {
        "-0".into()
    } else {
        format!("{n:e}")
    }
}

/// A JSON string literal.
pub fn json_string(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if u32::from(c) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", u32::from(c));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

// ---------------------------------------------------------------- generator

/// Text with the characters React escapes, `$` (Flight's escape), Unicode and newlines.
fn text() -> impl Strategy<Value = String> {
    prop_oneof![
        1 => Just(String::new()),
        8 => "[a-z $<>&\"'é✓\\n]{1,6}",
        1 => "\\n[a-z]{0,3}", // a leading newline (`<pre>`, `<textarea>`)
    ]
}

fn number() -> impl Strategy<Value = f64> {
    prop_oneof![
        4 => (-1000i32..1000).prop_map(f64::from),
        3 => -1e6f64..1e6,
        1 => prop_oneof![
            Just(f64::NAN),
            Just(f64::INFINITY),
            Just(f64::NEG_INFINITY),
            Just(-0.0),
            Just(1e21),
            Just(1e-7),
            Just(0.1 + 0.2),
        ],
    ]
}

/// Bigints: small, negative, zero and beyond 64 bits.
fn bigint() -> impl Strategy<Value = String> {
    prop_oneof![
        3 => any::<i64>().prop_map(|n| n.to_string()),
        1 => "-?[1-9][0-9]{19,40}",
        1 => Just("0".to_owned()),
    ]
}

/// Strings that exercise the value rules: numeric strings (for `isNaN` and `1 <= value`), URLs
/// (`javascript:` blocking, `data:` images), `<img>`'s `loading` and `fetchPriority`, and a
/// leading newline (`<pre>`, `<textarea>`).
fn special_string() -> impl Strategy<Value = String> {
    prop::sample::select(vec![
        "0",
        "1",
        "2",
        " 3 ",
        "0.5",
        "1e0",
        "0x10",
        "0X1f",
        "0b11",
        "0O7",
        "0x",
        "1.",
        ".5e1",
        "+2",
        "-0",
        "-1",
        "Infinity",
        "-Infinity",
        "infinity",
        "1_0",
        "\u{3000}4\u{FEFF}",
        "abc",
        "",
        "javascript:alert(1)",
        " \tJaVa\nScRiPt:x",
        "/a?b=1&c=2",
        "https://example.com",
        "java script:x",
        "lazy",
        "low",
        "data:x",
        "DaTa:y",
        "data",
        "\nx",
    ])
    .prop_map(str::to_owned)
}

fn prop_value(numeric: bool) -> BoxedStrategy<Value> {
    let bigint = if numeric {
        Just(Value::Null).boxed()
    } else {
        bigint().prop_map(Value::BigInt).boxed()
    };
    prop_oneof![
        4 => text().prop_map(Value::String),
        3 => special_string().prop_map(Value::String),
        2 => number().prop_map(Value::Number),
        1 => bigint,
        3 => any::<bool>().prop_map(Value::Bool),
        1 => Just(Value::Null),
        1 => Just(Value::Undefined),
    ]
    .boxed()
}

fn prop_name() -> impl Strategy<Value = String> {
    prop_oneof![
        8 => prop::sample::select(KNOWN_PROPS).prop_map(str::to_owned),
        1 => prop::sample::select(UNSAFE_PROPS).prop_map(str::to_owned),
        1 => "[a-zA-Z][a-zA-Z0-9-]{0,6}",
    ]
}

/// CSS property names covering React's naming rules: camelCase, vendor prefixes (`Webkit`,
/// `Moz`, `ms`), unitless properties, custom properties, escaping and non-ASCII case mapping.
const STYLE_NAMES: &[&str] = &[
    "color",
    "backgroundColor",
    "width",
    "margin",
    "fontSize",
    "lineHeight",
    "opacity",
    "zIndex",
    "flex",
    "flexGrow",
    "gridRow",
    "strokeWidth",
    "WebkitTransition",
    "WebkitLineClamp",
    "MozBoxFlex",
    "msTransform",
    "msFlex",
    "WebKitBoxFlexGroup",
    "MsTransform",
    "XMLHttp",
    "--gap",
    "--My-Var",
    "content",
    "a\"b<",
    "ÄpfelGröße",
    "ΟΔΟΣ",
];

fn style_value() -> impl Strategy<Value = Value> {
    let css = prop::sample::select(vec![" 1em ", "  ", "", "red", "calc(1px + 2%)", "'a'"])
        .prop_map(|s| Value::String(s.to_owned()));
    prop_oneof![
        3 => css,
        2 => text().prop_map(Value::String),
        3 => number().prop_map(Value::Number),
        1 => bigint().prop_map(Value::BigInt),
        1 => any::<bool>().prop_map(Value::Bool),
        1 => Just(Value::Null),
        1 => Just(Value::Undefined),
    ]
}

/// A `style` prop: usually an object, sometimes `null` or `undefined`.
fn style() -> impl Strategy<Value = Value> {
    let name = prop_oneof![
        4 => prop::sample::select(STYLE_NAMES).prop_map(str::to_owned),
        1 => "-?[a-zA-Z]{1,8}",
    ];
    let object = prop::collection::vec((name, style_value()), 0..5).prop_map(|mut entries| {
        let mut seen = std::collections::HashSet::new();
        entries.retain(|(name, _)| seen.insert(name.clone()));
        Value::Style(entries)
    });
    prop_oneof![
        6 => object,
        1 => Just(Value::Null),
        1 => Just(Value::Undefined),
    ]
}

fn props() -> impl Strategy<Value = Vec<(String, Value)>> {
    with_style(named_props(prop_name().boxed(), false))
}

/// Props with a `style` prop inserted at a random place, sometimes.
fn with_style(
    props: impl Strategy<Value = Vec<(String, Value)>>,
) -> impl Strategy<Value = Vec<(String, Value)>> {
    let with_style = (
        props,
        prop::option::weighted(0.3, style()),
        any::<prop::sample::Index>(),
    );
    with_style.prop_map(|(mut props, style, at)| {
        if let Some(style) = style {
            let at = at.index(props.len() + 1);
            props.insert(at, ("style".to_owned(), style));
        }
        props
    })
}

/// Props with these names; `select`: a `<select>`'s, whose `value` and `defaultValue` can be
/// arrays.
fn named_props(
    name: BoxedStrategy<String>,
    select: bool,
) -> impl Strategy<Value = Vec<(String, Value)>> {
    prop::collection::vec(name, 0..6).prop_flat_map(move |mut names| {
        let mut seen = std::collections::HashSet::new();
        names.retain(|n| {
            !matches!(n.as_str(), "key" | "ref" | "style" | "children") && seen.insert(n.clone())
        });
        names
            .into_iter()
            .map(|name| {
                let value = match name.as_str() {
                    "dangerouslySetInnerHTML" => inner_html().boxed(),
                    "src" | "srcSet" | "loading" | "fetchPriority" => prop_oneof![
                        3 => image_value(),
                        1 => prop_value(false),
                    ]
                    .boxed(),
                    "value" | "defaultValue" if select => prop_oneof![
                        6 => prop_value(false),
                        1 => prop::collection::vec(prop_value(false), 0..3).prop_map(Value::Array),
                    ]
                    .boxed(),
                    name => prop_value(NUMERIC_PROPS.contains(&name)),
                };
                (Just(name), value)
            })
            .collect::<Vec<_>>()
    })
}

/// Values for `<img>`'s preload rules: URLs (`data:` in any case, empty), and `loading` and
/// `fetchPriority` values.
fn image_value() -> impl Strategy<Value = Value> {
    prop::sample::select(vec![
        "/x.png", "", "data:x", "DaTa:y", "data", "datax:", "lazy", "eager", "low", "high",
    ])
    .prop_map(|s| Value::String(s.to_owned()))
}

/// `dangerouslySetInnerHTML`: usually `{ __html }` (markup, `\n`-prefixed, any primitive), and
/// sometimes a value React throws on.
fn inner_html() -> impl Strategy<Value = Value> {
    let html = prop_oneof![
        4 => prop::sample::select(vec!["<b>x</b>", "\nx", "", "a&b"])
            .prop_map(|s| Value::String(s.to_owned())),
        2 => prop_value(false),
    ];
    prop_oneof![
        8 => html.prop_map(|v| Value::Html(Box::new(v))),
        1 => Just(Value::Null),
        1 => prop_value(false),
    ]
}

/// The props of an element in [`SPECIAL_TAGS`]: mostly the ones its rules treat specially.
fn special_element_props(tag: &'static str) -> impl Strategy<Value = Vec<(String, Value)>> {
    let special = prop::sample::select(special_props(tag)).prop_map(str::to_owned);
    let name = prop_oneof![3 => special, 1 => prop_name()].boxed();
    with_style(named_props(name, tag == "select"))
}

/// The nodes a template can't know until runtime, as `Op::Child` reads them: primitives and
/// arrays of them.
fn primitive() -> impl Strategy<Value = Tree> {
    prop_oneof![
        6 => text().prop_map(Tree::Text),
        2 => number().prop_map(Tree::Number),
        1 => bigint().prop_map(Tree::BigInt),
        1 => Just(Tree::Null),
        1 => Just(Tree::Undefined),
        1 => any::<bool>().prop_map(Tree::Bool),
    ]
}

fn hole() -> impl Strategy<Value = Tree> {
    primitive()
        .prop_recursive(3, 16, 4, |inner| {
            prop::collection::vec(inner, 0..4).prop_map(Tree::Array)
        })
        .prop_map(|node| Tree::Hole(Box::new(node)))
}

/// Keys, including one Flight escapes (`$`).
fn key() -> impl Strategy<Value = Option<String>> {
    prop::option::weighted(0.3, "[a-z0-9$]{0,3}")
}

/// An element with these parts; `runtime` (or a special tag) makes it an element value, which
/// may place `children` anywhere among its props.
fn element(
    tag: &'static str,
    props: Vec<(String, Value)>,
    (spread, runtime): (bool, bool),
    key: Option<String>,
    children_at: prop::sample::Index,
    children: Vec<Tree>,
) -> Tree {
    let runtime = runtime || SPECIAL_TAGS.contains(&tag);
    let children_at =
        (runtime && children_at.index(4) == 0).then(|| children_at.index(props.len() + 1));
    Tree::Element(Element {
        tag,
        key,
        props,
        spread,
        runtime,
        children_at,
        children,
    })
}

/// An element with generic rules: a template element or an element value.
fn generic_element(
    tags: &'static [&'static str],
    children: impl Strategy<Value = Vec<Tree>>,
) -> impl Strategy<Value = Tree> {
    (
        prop::sample::select(tags),
        props(),
        (any::<bool>(), prop::bool::weighted(0.3)),
        key(),
        any::<prop::sample::Index>(),
        children,
    )
        .prop_map(|(tag, props, flags, key, at, children)| {
            element(tag, props, flags, key, at, children)
        })
}

/// An element with rules of its own (always an element value). Void ones rarely get children,
/// which React rejects.
fn special_element(children: BoxedStrategy<Vec<Tree>>) -> impl Strategy<Value = Tree> {
    prop::sample::select(SPECIAL_TAGS).prop_flat_map(move |tag| {
        let void = matches!(tag, "input" | "img" | "source" | "menuitem");
        let children = if void {
            prop_oneof![9 => Just(vec![]), 1 => children.clone()].boxed()
        } else {
            children.clone()
        };
        (
            special_element_props(tag),
            key(),
            any::<prop::sample::Index>(),
            children,
        )
            .prop_map(move |(props, key, at, children)| {
                element(tag, props, (false, true), key, at, children)
            })
    })
}

/// A `<select>` with options: directly, in `<optgroup>`s and other elements, and behind elements
/// that reset the select's context (`<table>`, `<svg>`, `<noscript>`).
fn select(inner: BoxedStrategy<Tree>) -> impl Strategy<Value = Tree> {
    // Option texts and select values from one small pool, so they often match: `"1"` matches
    // the number 1, the bigint 1, and option text `1` from a number child.
    let option_child = prop_oneof![
        4 => prop_oneof![
            Just(Tree::Text("a".to_owned())),
            Just(Tree::Text("1".to_owned())),
            Just(Tree::Number(1.0)),
            Just(Tree::BigInt("1".to_owned())),
            Just(Tree::Null),
            Just(Tree::Bool(true)),
        ],
        2 => primitive(),
        1 => inner,
    ];
    let pool = || {
        prop::sample::select(vec![
            Value::String("a".to_owned()),
            Value::String("1".to_owned()),
            Value::String("a1".to_owned()),
            Value::String("11".to_owned()),
            Value::Number(1.0),
            Value::BigInt("1".to_owned()),
        ])
    };
    let select_value = prop_oneof![
        3 => pool(),
        1 => prop::collection::vec(pool(), 0..3).prop_map(Value::Array),
    ];
    let option = (
        special_element_props("option"),
        prop::collection::vec(option_child, 0..3),
    )
        .prop_map(|(props, children)| {
            Tree::Element(Element {
                tag: "option",
                key: None,
                props,
                spread: false,
                runtime: true,
                children_at: None,
                children,
            })
        })
        .boxed();
    let wrapped = (
        prop::sample::select(vec!["optgroup", "div", "table", "svg", "noscript"]),
        prop::collection::vec(option.clone(), 0..3),
    )
        .prop_map(|(tag, children)| {
            Tree::Element(Element {
                tag,
                key: None,
                props: vec![],
                spread: false,
                runtime: true,
                children_at: None,
                children,
            })
        });
    (
        special_element_props("select"),
        prop::option::weighted(
            0.7,
            (
                prop::sample::select(vec!["value", "defaultValue"]),
                select_value,
            ),
        ),
        prop::collection::vec(prop_oneof![3 => option, 1 => wrapped], 0..4),
    )
        .prop_map(|(mut props, value, children)| {
            if let Some((name, value)) = value {
                props.retain(|(n, _)| n != "value" && n != "defaultValue");
                props.insert(0, (name.to_owned(), value));
            }
            Tree::Element(Element {
                tag: "select",
                key: None,
                props,
                spread: false,
                runtime: true,
                children_at: None,
                children,
            })
        })
}

/// `<pre>`, `<listing>` or `<textarea>` with one text child that may start with a newline, which
/// React doubles.
fn leading_newline() -> impl Strategy<Value = Tree> {
    (
        prop::sample::select(vec!["pre", "listing", "textarea", "div"]),
        "\\n?[a-z\\n]{0,3}",
    )
        .prop_map(|(tag, text)| {
            Tree::Element(Element {
                tag,
                key: None,
                props: vec![],
                spread: false,
                runtime: true,
                children_at: None,
                children: vec![Tree::Text(text)],
            })
        })
}

/// An `<img>` with preload props, in an element that may stop React's preloads (`<picture>`,
/// `<noscript>`) or not.
fn image_in_scope() -> impl Strategy<Value = Tree> {
    (
        prop::sample::select(vec!["picture", "noscript", "div", "span"]),
        special_element_props("img"),
    )
        .prop_map(|(tag, props)| {
            let img = Tree::Element(Element {
                tag: "img",
                key: None,
                props,
                spread: false,
                runtime: true,
                children_at: None,
                children: vec![],
            });
            Tree::Element(Element {
                tag,
                key: None,
                props: vec![],
                spread: false,
                runtime: true,
                children_at: None,
                children: vec![img],
            })
        })
}

/// A random tree: elements (generic, void, custom and special; template elements and element
/// values) with props; text, numbers, bigints, `null`, `undefined` and booleans; arrays; and
/// runtime holes.
pub fn tree() -> impl Strategy<Value = Tree> {
    let leaf = prop_oneof![
        11 => primitive(),
        3 => hole(),
        1 => generic_element(VOID_TAGS, Just(vec![])),
        1 => special_element(prop::collection::vec(primitive(), 0..3).boxed()),
    ];
    leaf.prop_recursive(4, 48, 5, |inner| {
        let children = prop::collection::vec(inner.clone(), 0..5).boxed();
        prop_oneof![
            4 => generic_element(TAGS, children.clone()),
            2 => special_element(children),
            1 => select(inner.clone()),
            1 => image_in_scope(),
            1 => leading_newline(),
            1 => prop::collection::vec(inner, 0..4).prop_map(Tree::Array),
        ]
    })
}

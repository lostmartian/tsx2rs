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
    /// only what `Op::Child` takes (no elements).
    Hole(Box<Self>),
    /// A host element.
    Element(Element),
}

/// A host element (`<div>`, `<a>`, `<br>`, …).
#[derive(Clone, Debug)]
pub struct Element {
    /// The tag.
    pub tag: &'static str,
    /// Props in order (unique names): React prop name and value. A `style` prop's value is a
    /// [`Value::Style`], `null` or `undefined`.
    pub props: Vec<(String, Value)>,
    /// Whether the props come from a spread (`<div {...props}>`): the compiler doesn't know
    /// their names, so they're lowered to one `Op::Attributes`. Otherwise each prop is an
    /// `Op::Prop` (or `Op::Value` for props React doesn't write).
    pub spread: bool,
    /// Children (none for void elements).
    pub children: Vec<Tree>,
}

impl Element {
    /// Whether the tag is a void element (no children, written `<tag/>`).
    #[must_use]
    pub fn is_void(&self) -> bool {
        VOID_TAGS.contains(&self.tag)
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
}

/// Tags whose props follow React DOM's generic rules, plus `a` (the only difference: `href=""`
/// is written). Elements with rules of their own (`input`, `select`, `img`, `script`, …) get
/// their own tests with the engine support for them.
pub const TAGS: &[&str] = &[
    "div", "span", "p", "ul", "li", "b", "i", "section", "h1", "a", "svg", "g", "path", "td",
];
/// Void tags the generator uses (React's `pushSelfClosing`).
pub const VOID_TAGS: &[&str] = &["br", "hr", "wbr"];

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
];

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

/// Strings that exercise the value rules: numeric strings (for `isNaN` and `1 <= value`) and
/// URLs (`javascript:` blocking).
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
    let with_style = (
        named_props(),
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

fn named_props() -> impl Strategy<Value = Vec<(String, Value)>> {
    prop::collection::vec(prop_name(), 0..6).prop_flat_map(|mut names| {
        let mut seen = std::collections::HashSet::new();
        names.retain(|n| {
            !matches!(n.as_str(), "key" | "ref" | "style" | "children") && seen.insert(n.clone())
        });
        names
            .into_iter()
            .map(|name| {
                let numeric = NUMERIC_PROPS.contains(&name.as_str());
                (Just(name), prop_value(numeric))
            })
            .collect::<Vec<_>>()
    })
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

/// A random tree: elements (normal and void) with props; text, numbers, bigints, `null`,
/// `undefined` and booleans; arrays; and runtime holes.
pub fn tree() -> impl Strategy<Value = Tree> {
    let leaf = prop_oneof![
        11 => primitive(),
        3 => hole(),
        1 => (prop::sample::select(VOID_TAGS), props(), any::<bool>()).prop_map(
            |(tag, props, spread)| Tree::Element(Element { tag, props, spread, children: vec![] })
        ),
    ];
    leaf.prop_recursive(4, 48, 5, |inner| {
        prop_oneof![
            4 => (
                prop::sample::select(TAGS),
                props(),
                any::<bool>(),
                prop::collection::vec(inner.clone(), 0..5),
            )
                .prop_map(|(tag, props, spread, children)| {
                    Tree::Element(Element {
                        tag,
                        props,
                        spread,
                        children,
                    })
                }),
            1 => prop::collection::vec(inner, 0..4).prop_map(Tree::Array),
        ]
    })
}

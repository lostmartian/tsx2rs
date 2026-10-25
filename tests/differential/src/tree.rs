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

/// A host element (`<div>`, `<input>`, …).
#[derive(Clone, Debug)]
pub struct Element {
    /// The tag.
    pub tag: &'static str,
    /// Props in order: React prop name and value.
    pub attributes: Vec<(&'static str, Attribute)>,
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

/// An attribute value.
#[derive(Clone, Debug)]
pub enum Attribute {
    /// A string (for string props such as `className`).
    String(String),
    /// A boolean (for boolean props such as `hidden`).
    Bool(bool),
    /// `null`
    Null,
    /// `undefined`
    Undefined,
}

/// Tags the generator uses. Raw-text and special elements (`script`, `style`, `title`,
/// `textarea`, `select`, `option`) have extra React rules and get their own tests later.
pub const TAGS: &[&str] = &[
    "div", "span", "p", "ul", "li", "b", "i", "section", "h1", "button",
];
/// Void tags the generator uses.
pub const VOID_TAGS: &[&str] = &["br", "img", "input"];
/// String props and their HTML attribute names.
pub const STRING_PROPS: &[(&str, &str)] = &[
    ("className", "class"),
    ("id", "id"),
    ("title", "title"),
    ("data-x", "data-x"),
];
/// Boolean props and their HTML attribute names.
pub const BOOLEAN_PROPS: &[(&str, &str)] = &[("hidden", "hidden"), ("disabled", "disabled")];

/// The HTML attribute name of a React prop the generator uses.
#[must_use]
pub fn html_name(prop: &str) -> &'static str {
    STRING_PROPS
        .iter()
        .chain(BOOLEAN_PROPS)
        .find(|(p, _)| *p == prop)
        .map_or("", |(_, h)| h)
}

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
            Self::Number(n) => {
                out.push_str(r#"{"n":"#);
                json_string(out, &number_repr(*n));
                out.push('}');
            }
            Self::Null => out.push_str("null"),
            Self::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Self::Undefined => out.push_str(r#"{"u":1}"#),
            Self::BigInt(digits) => {
                out.push_str(r#"{"b":"#);
                json_string(out, digits);
                out.push('}');
            }
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
                for (i, (name, value)) in e.attributes.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    out.push('[');
                    json_string(out, name);
                    out.push(',');
                    match value {
                        Attribute::String(s) => json_string(out, s),
                        Attribute::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
                        Attribute::Null => out.push_str("null"),
                        Attribute::Undefined => out.push_str(r#"{"u":1}"#),
                    }
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

fn attributes() -> impl Strategy<Value = Vec<(&'static str, Attribute)>> {
    let string_value = prop_oneof![
        6 => text().prop_map(Attribute::String),
        1 => Just(Attribute::Null),
        1 => Just(Attribute::Undefined),
    ];
    let boolean_value = prop_oneof![
        3 => any::<bool>().prop_map(Attribute::Bool),
        1 => Just(Attribute::Null),
        1 => Just(Attribute::Undefined),
    ];
    let string_attrs = prop::sample::subsequence(STRING_PROPS.to_vec(), 0..=STRING_PROPS.len())
        .prop_flat_map(move |props| {
            props
                .into_iter()
                .map(|(p, _)| (Just(p), string_value.clone()))
                .collect::<Vec<_>>()
        });
    let boolean_attrs = prop::sample::subsequence(BOOLEAN_PROPS.to_vec(), 0..=BOOLEAN_PROPS.len())
        .prop_flat_map(move |props| {
            props
                .into_iter()
                .map(|(p, _)| (Just(p), boolean_value.clone()))
                .collect::<Vec<_>>()
        });
    (string_attrs, boolean_attrs).prop_map(|(mut a, b)| {
        a.extend(b);
        a
    })
}

/// Bigints: small, negative, zero and beyond 64 bits.
fn bigint() -> impl Strategy<Value = String> {
    prop_oneof![
        3 => any::<i64>().prop_map(|n| n.to_string()),
        1 => "-?[1-9][0-9]{19,40}",
        1 => Just("0".to_owned()),
    ]
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

/// A random tree: elements (normal and void) with attributes; text, numbers, bigints, `null`,
/// `undefined` and booleans; arrays; and runtime holes.
pub fn tree() -> impl Strategy<Value = Tree> {
    let leaf = prop_oneof![
        11 => primitive(),
        3 => hole(),
        1 => (prop::sample::select(VOID_TAGS), attributes())
            .prop_map(|(tag, attributes)| Tree::Element(Element { tag, attributes, children: vec![] })),
    ];
    leaf.prop_recursive(4, 48, 5, |inner| {
        prop_oneof![
            4 => (
                prop::sample::select(TAGS),
                attributes(),
                prop::collection::vec(inner.clone(), 0..5),
            )
                .prop_map(|(tag, attributes, children)| {
                    Tree::Element(Element {
                        tag,
                        attributes,
                        children,
                    })
                }),
            1 => prop::collection::vec(inner, 0..4).prop_map(Tree::Array),
        ]
    })
}

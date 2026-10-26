//! Lowers a generated tree to a template program and its input, the way the compiler will for
//! host elements: element structure becomes static chunks, and every text, number and prop value
//! is read from the input, so the engine (not this file) does all escaping, separating and
//! attribute rules.

use crate::tree::{Element, Tree, Value, json_string};
use tsx2rs_ir::attribute::{ElementRules, Prop, classify};
use tsx2rs_ir::input::{InputWriter, tag};
use tsx2rs_ir::{ChunkId, FormatError, Op, Program};

/// A tree's program and the input that renders it.
#[derive(Debug)]
pub struct Lowered {
    /// The program.
    pub program: Program,
    /// The input.
    pub input: Vec<u8>,
}

/// Lowers a tree.
///
/// # Errors
///
/// If the program fails validation (a bug in this lowering).
pub fn lower(tree: &Tree) -> Result<Lowered, FormatError> {
    let mut lowering = Lowering::default();
    lowering.node(tree);
    Ok(Lowered {
        program: Program::new(lowering.chunks, lowering.ops)?,
        input: lowering.input.finish(),
    })
}

#[derive(Default)]
struct Lowering {
    chunks: Vec<Box<[u8]>>,
    ops: Vec<Op>,
    input: InputWriter,
}

impl Lowering {
    fn chunk(&mut self, s: &str) -> ChunkId {
        let index = self
            .chunks
            .iter()
            .position(|c| **c == *s.as_bytes())
            .unwrap_or_else(|| {
                self.chunks.push(s.as_bytes().into());
                self.chunks.len() - 1
            });
        ChunkId(u32::try_from(index).unwrap_or(u32::MAX))
    }

    /// Static markup (never text at its edges).
    fn markup(&mut self, html: &str, flight: &str) {
        let (html, flight) = (self.chunk(html), self.chunk(flight));
        self.ops.push(Op::Static {
            html,
            flight,
            text_start: false,
            text_end: false,
        });
    }

    fn node(&mut self, tree: &Tree) {
        match tree {
            Tree::Text(s) => {
                self.ops.push(Op::Text);
                self.input.string(s);
            }
            Tree::Number(n) => {
                self.ops.push(Op::Number);
                self.input.number(*n);
            }
            Tree::Null => self.markup("", "null"),
            Tree::Bool(b) => self.markup("", if *b { "true" } else { "false" }),
            Tree::Undefined => self.markup("", r#""$undefined""#),
            // A literal bigint: a static text node, as the compiler lowers `{12n}`.
            Tree::BigInt(digits) => {
                let (html, flight) = (self.chunk(digits), self.chunk(&format!(r#""$n{digits}""#)));
                self.ops.push(Op::Static {
                    html,
                    flight,
                    text_start: true,
                    text_end: true,
                });
            }
            Tree::Array(nodes) => {
                self.markup("", "[");
                for (i, node) in nodes.iter().enumerate() {
                    if i > 0 {
                        self.markup("", ",");
                    }
                    self.node(node);
                }
                self.markup("", "]");
            }
            Tree::Hole(node) => {
                self.ops.push(Op::Child);
                self.child_input(node);
            }
            Tree::Element(e) => self.element(e),
        }
    }

    /// A node as `Op::Child` reads it.
    fn child_input(&mut self, node: &Tree) {
        match node {
            Tree::Text(s) => {
                self.input.tag(tag::STRING).string(s);
            }
            Tree::Number(n) => {
                self.input.tag(tag::NUMBER).number(*n);
            }
            Tree::BigInt(digits) => {
                self.input.tag(tag::BIGINT).string(digits);
            }
            Tree::Null => {
                self.input.tag(tag::NULL);
            }
            Tree::Undefined => {
                self.input.tag(tag::UNDEFINED);
            }
            Tree::Bool(b) => {
                self.input.tag(if *b { tag::TRUE } else { tag::FALSE });
            }
            Tree::Array(nodes) => {
                self.input.tag(tag::ARRAY).count(nodes.len());
                for node in nodes {
                    self.child_input(node);
                }
            }
            Tree::Hole(node) => self.child_input(node),
            Tree::Element(_) => unreachable!("the generator puts no elements in holes"),
        }
    }

    /// `<tag props>children</tag>` and `["$","tag",null,{props}]`, props in React's order:
    /// as written, then `children` (absent for none, the child for one, an array for several),
    /// as `createElement(tag, props, ...children)` builds them.
    ///
    /// Props are lowered the way the compiler lowers them: one `Op::Attributes` for a spread
    /// (names known only at runtime); otherwise, per prop, a static Flight key and an `Op::Prop`
    /// with the name and kind from `classify`, or an `Op::Value` (Flight only) for a prop React
    /// doesn't write.
    fn element(&mut self, e: &Element) {
        let rules = if e.tag == "a" {
            ElementRules::Anchor
        } else {
            ElementRules::Generic
        };
        let has_children = !e.is_void() && !e.children.is_empty();
        self.markup(
            &format!("<{}", e.tag),
            &format!(r#"["$","{}",null,{{"#, e.tag),
        );
        if e.spread {
            self.ops.push(Op::Attributes {
                rules,
                comma_after: has_children,
            });
            self.input.count(e.props.len());
            for (name, value) in &e.props {
                self.input.string(name);
                self.value(value);
            }
        } else {
            for (i, (name, value)) in e.props.iter().enumerate() {
                let mut key = String::from(if i > 0 { "," } else { "" });
                json_string(&mut key, name);
                key.push(':');
                self.markup("", &key);
                match classify(name, rules) {
                    Prop::Attribute { name, kind } => {
                        let name = self.chunk(name);
                        self.ops.push(Op::Prop { name, kind });
                    }
                    Prop::Ignored | Prop::Style | Prop::Content | Prop::Ref => {
                        self.ops.push(Op::Value);
                    }
                }
                self.value(value);
            }
            if has_children && !e.props.is_empty() {
                self.markup("", ",");
            }
        }
        if e.is_void() {
            self.markup("/>", "}]");
            return;
        }
        match e.children.as_slice() {
            [] => self.markup(">", ""),
            [only] => {
                self.markup(">", r#""children":"#);
                self.node(only);
            }
            children => {
                self.markup(">", r#""children":["#);
                for (i, child) in children.iter().enumerate() {
                    if i > 0 {
                        self.markup("", ",");
                    }
                    self.node(child);
                }
                self.markup("", "]");
            }
        }
        self.markup(&format!("</{}>", e.tag), "}]");
    }

    /// A prop value as `Op::Prop`, `Op::Attributes` and `Op::Value` read it.
    fn value(&mut self, value: &Value) {
        match value {
            Value::String(s) => {
                self.input.tag(tag::STRING).string(s);
            }
            Value::Number(n) => {
                self.input.tag(tag::NUMBER).number(*n);
            }
            Value::BigInt(digits) => {
                self.input.tag(tag::BIGINT).string(digits);
            }
            Value::Bool(b) => {
                self.input.tag(if *b { tag::TRUE } else { tag::FALSE });
            }
            Value::Null => {
                self.input.tag(tag::NULL);
            }
            Value::Undefined => {
                self.input.tag(tag::UNDEFINED);
            }
        }
    }
}

//! Lowers a generated tree to a template program and its input, the way the compiler will for
//! host elements: element structure becomes static chunks, and every text, number and attribute
//! value is read from the input, so the engine (not this file) does all escaping and separating.

use crate::tree::{Attribute, Element, Tree, html_name};
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

    /// `<tag attrs>children</tag>` and `["$","tag",null,{props}]`, props in React's order:
    /// attributes as written, then `children` (absent for none, the child for one, an array for
    /// several), as `createElement(tag, props, ...children)` builds them.
    fn element(&mut self, e: &Element) {
        self.markup(
            &format!("<{}", e.tag),
            &format!(r#"["$","{}",null,{{"#, e.tag),
        );
        for (i, (prop, value)) in e.attributes.iter().enumerate() {
            let comma = if i > 0 { "," } else { "" };
            self.markup("", &format!(r#"{comma}"{prop}":"#));
            let name = self.chunk(html_name(prop));
            match value {
                Attribute::String(s) => {
                    self.ops.push(Op::Attribute { name });
                    self.input.tag(tag::STRING).string(s);
                }
                Attribute::Bool(b) => {
                    self.ops.push(Op::BooleanAttribute { name });
                    self.input.tag(if *b { tag::TRUE } else { tag::FALSE });
                }
                Attribute::Null | Attribute::Undefined => {
                    let t = if matches!(value, Attribute::Null) {
                        tag::NULL
                    } else {
                        tag::UNDEFINED
                    };
                    let is_boolean = crate::tree::BOOLEAN_PROPS.iter().any(|(p, _)| p == prop);
                    self.ops.push(if is_boolean {
                        Op::BooleanAttribute { name }
                    } else {
                        Op::Attribute { name }
                    });
                    self.input.tag(t);
                }
            }
        }
        if e.is_void() {
            self.markup("/>", "}]");
            return;
        }
        let comma = if e.attributes.is_empty() { "" } else { "," };
        match e.children.as_slice() {
            [] => self.markup(">", ""),
            [only] => {
                self.markup(">", &format!(r#"{comma}"children":"#));
                self.node(only);
            }
            children => {
                self.markup(">", &format!(r#"{comma}"children":["#));
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
}

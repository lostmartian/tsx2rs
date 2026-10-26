//! Lowers a generated tree to a template program and its input, the way the compiler will for
//! host elements: element structure becomes static chunks, and every text, number and prop value
//! is read from the input, so the engine (not this file) does all escaping, separating and
//! attribute rules.

use crate::tree::{Element, Link, Tree, Value, json_string};
use tsx2rs_ir::attribute::{ElementRules, Prop, classify, is_custom_element};
use tsx2rs_ir::input::{InputWriter, tag};
use tsx2rs_ir::{ChunkId, FormatError, Op, Program, TemplateId};

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
    let mut lowering = Lowering {
        templates: vec![Vec::new()],
        ..Lowering::default()
    };
    lowering.node(tree);
    Ok(Lowered {
        program: Program::with_templates(lowering.chunks, lowering.templates)?,
        input: lowering.input.finish(),
    })
}

#[derive(Default)]
struct Lowering {
    chunks: Vec<Box<[u8]>>,
    /// Every template; ops go to `current`.
    templates: Vec<Vec<Op>>,
    current: usize,
    input: InputWriter,
}

impl Lowering {
    fn op(&mut self, op: Op) {
        self.templates[self.current].push(op);
    }

    /// Lowers a compiled component's output `tree` as a new template (its input written in
    /// place), and returns its id.
    ///
    /// React's Flight server wraps a component's root element in an array when the element has
    /// a key and the component doesn't (`implicitSlot` in `renderElement`). The generated
    /// components have no keys, so a keyed root element is wrapped; the runtime computes this
    /// from the call chain, and the template writes it as plain Flight (ADR-0008).
    fn template(&mut self, tree: &Tree) -> TemplateId {
        let id = self.templates.len();
        self.templates.push(Vec::new());
        let caller = std::mem::replace(&mut self.current, id);
        let wrap = matches!(tree, Tree::Element(e) if e.key.is_some());
        if wrap {
            self.markup("", "[");
        }
        self.node(tree);
        if wrap {
            self.markup("", "]");
        }
        self.current = caller;
        TemplateId(u32::try_from(id).unwrap_or(u32::MAX))
    }

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
        self.op(Op::Static {
            html,
            flight,
            text_start: false,
            text_end: false,
        });
    }

    fn node(&mut self, tree: &Tree) {
        match tree {
            Tree::Text(s) => {
                self.op(Op::Text);
                self.input.string(s);
            }
            Tree::Number(n) => {
                self.op(Op::Number);
                self.input.number(*n);
            }
            Tree::Null => self.markup("", "null"),
            Tree::Bool(b) => self.markup("", if *b { "true" } else { "false" }),
            Tree::Undefined => self.markup("", r#""$undefined""#),
            // A literal bigint: a static text node, as the compiler lowers `{12n}`.
            Tree::BigInt(digits) => {
                let (html, flight) = (self.chunk(digits), self.chunk(&format!(r#""$n{digits}""#)));
                self.op(Op::Static {
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
                self.op(Op::Child);
                self.child_input(node);
            }
            // A `Child` hole starts in the body's context (ADR-0009), so the compiler lowers an
            // `<svg>` with anything known only at runtime inside as an element value.
            Tree::Element(e) if e.runtime || (e.tag == "svg" && has_runtime_part(tree)) => {
                self.op(Op::Child);
                self.child_input(tree);
            }
            Tree::Element(e) => self.element(e),
            Tree::Component(inner, Link::Call) => {
                let at = self.templates[self.current].len();
                self.op(Op::Child); // placeholder: the call goes here, before its template's ops
                let template = self.template(inner);
                self.templates[self.current][at] = Op::Call { template };
            }
            Tree::Component(_, Link::Hole) => {
                self.op(Op::Child);
                self.child_input(tree);
            }
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
            Tree::Element(e) => self.element_input(e),
            Tree::Component(inner, _) => {
                // The template's index goes before its input, so reserve it: templates are
                // numbered in creation order.
                let id = self.templates.len();
                self.input.tag(tag::TEMPLATE).count(id);
                self.template(inner);
            }
        }
    }

    /// An element node (ADR-0007): type, key, props in order with the `children` entry where
    /// `children` is, then the children node.
    fn element_input(&mut self, e: &Element) {
        self.input.tag(tag::ELEMENT).string(e.tag);
        match &e.key {
            Some(key) => self.input.tag(tag::STRING).string(key),
            None => self.input.tag(tag::NULL),
        };
        let children = e.children_value();
        let children_at = children
            .as_ref()
            .map(|_| e.children_at.unwrap_or(e.props.len()));
        self.input
            .count(e.props.len() + usize::from(children.is_some()));
        for (i, (name, value)) in e.props.iter().enumerate() {
            if children_at == Some(i) {
                self.input.string("children");
            }
            self.input.string(name);
            self.value(value);
        }
        if children_at == Some(e.props.len()) {
            self.input.string("children");
        }
        if let Some(children) = &children {
            self.child_input(children);
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
        } else if is_custom_element(e.tag) {
            ElementRules::Custom
        } else {
            ElementRules::Generic
        };
        let has_children = !e.is_void() && !e.children.is_empty();
        let mut key = String::new();
        match &e.key {
            // Flight escapes a leading `$` (`escapeStringValue`).
            Some(k) => json_string(
                &mut key,
                &if k.starts_with('$') {
                    format!("${k}")
                } else {
                    k.clone()
                },
            ),
            None => key.push_str("null"),
        }
        self.markup(
            &format!("<{}", e.tag),
            &format!(r#"["$","{}",{key},{{"#, e.tag),
        );
        if e.spread {
            self.op(Op::Attributes {
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
                        self.op(Op::Prop { name, kind });
                    }
                    Prop::Style => self.op(Op::Style),
                    Prop::Ignored | Prop::Content | Prop::Ref => {
                        self.op(Op::Value);
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

    /// A prop value as `Op::Prop`, `Op::Attributes`, `Op::Style` and `Op::Value` read it.
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
            Value::Style(entries) => {
                self.input.tag(tag::OBJECT).count(entries.len());
                for (name, value) in entries {
                    self.input.string(name);
                    self.value(value);
                }
            }
            Value::Array(items) => {
                self.input.tag(tag::ARRAY).count(items.len());
                for item in items {
                    self.value(item);
                }
            }
            Value::Html(inner) => {
                self.input.tag(tag::OBJECT).count(1).string("__html");
                self.value(inner);
            }
        }
    }
}

/// Whether the tree holds anything known only at runtime: a hole, an element value or a compiled
/// component.
fn has_runtime_part(tree: &Tree) -> bool {
    match tree {
        Tree::Hole(_) | Tree::Component(..) => true,
        Tree::Element(e) => e.runtime || e.children.iter().any(has_runtime_part),
        Tree::Array(nodes) => nodes.iter().any(has_runtime_part),
        _ => false,
    }
}

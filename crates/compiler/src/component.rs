//! Component elements in a module's JSX: `<Gallery images={…} />`, `<Link href="/">…</Link>`.
//!
//! Only elements whose tag is a component (a capitalized identifier or a member expression, as
//! React decides) are collected; host elements (`<div>`) are not. Each use records its props as
//! written: a literal (with its exact JSON, so it can become a static Flight chunk), an expression
//! (read at runtime), or a spread.

use oxc_ast::ast::{
    Expression, JSXAttributeItem, JSXAttributeName, JSXAttributeValue, JSXChild, JSXElement,
    JSXElementName, JSXExpression, JSXMemberExpressionObject, Program, UnaryOperator,
};
use oxc_ast_visit::{Visit, walk};

/// One component element.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ComponentUse {
    /// The local binding the tag refers to (`Link`, or `ui` in `<ui.Button>`).
    pub name: String,
    /// For `<ui.Button>`: `Button` (nested members joined with `.`).
    pub member: Option<String>,
    /// Byte offsets of the opening tag.
    pub span: (u32, u32),
    /// Props in source order.
    pub props: Vec<Prop>,
    /// Whether the element has children (after JSX whitespace rules).
    pub has_children: bool,
}

/// A prop as written on the element.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prop {
    /// The prop name (`className`, `aria-label`); empty for a spread.
    pub name: String,
    /// Its value.
    pub value: PropValue,
}

/// A prop's value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PropValue {
    /// A literal: its JSON (`"relative h-full"`, `true`, `42`, `null`).
    Literal(String),
    /// Any other expression: its source text.
    Expression(String),
    /// `{...props}`: the spread expression's source text.
    Spread(String),
}

/// Collects the component elements of a module, in source order.
#[must_use]
pub fn collect(program: &Program<'_>) -> Vec<ComponentUse> {
    let mut collector = Collector {
        source: program.source_text,
        uses: Vec::new(),
    };
    collector.visit_program(program);
    collector.uses
}

struct Collector<'s> {
    source: &'s str,
    uses: Vec<ComponentUse>,
}

impl<'a> Visit<'a> for Collector<'_> {
    fn visit_jsx_element(&mut self, element: &JSXElement<'a>) {
        let opening = &element.opening_element;
        let tag = match &opening.name {
            JSXElementName::IdentifierReference(id) => Some((id.name.as_str().to_owned(), None)),
            JSXElementName::MemberExpression(member) => {
                let mut path = vec![member.property.name.as_str().to_owned()];
                let mut object = &member.object;
                loop {
                    match object {
                        JSXMemberExpressionObject::MemberExpression(inner) => {
                            path.push(inner.property.name.as_str().to_owned());
                            object = &inner.object;
                        }
                        JSXMemberExpressionObject::IdentifierReference(id) => {
                            path.reverse();
                            break Some((id.name.as_str().to_owned(), Some(path.join("."))));
                        }
                        JSXMemberExpressionObject::ThisExpression(_) => break None,
                    }
                }
            }
            JSXElementName::Identifier(_)
            | JSXElementName::NamespacedName(_)
            | JSXElementName::ThisExpression(_) => None,
        };
        if let Some((name, member)) = tag {
            let props = opening.attributes.iter().map(|a| self.prop(a)).collect();
            self.uses.push(ComponentUse {
                name,
                member,
                span: (opening.span.start, opening.span.end),
                props,
                has_children: element.children.iter().any(is_child),
            });
        }
        walk::walk_jsx_element(self, element);
    }
}

impl Collector<'_> {
    fn text(&self, span: oxc_span::Span) -> String {
        self.source
            .get(span.start as usize..span.end as usize)
            .unwrap_or_default()
            .to_owned()
    }

    fn prop(&self, item: &JSXAttributeItem<'_>) -> Prop {
        match item {
            JSXAttributeItem::SpreadAttribute(spread) => Prop {
                name: String::new(),
                value: PropValue::Spread(self.text(oxc_span::GetSpan::span(&spread.argument))),
            },
            JSXAttributeItem::Attribute(attribute) => {
                let name = match &attribute.name {
                    JSXAttributeName::Identifier(id) => id.name.as_str().to_owned(),
                    JSXAttributeName::NamespacedName(n) => {
                        format!("{}:{}", n.namespace.name.as_str(), n.name.name.as_str())
                    }
                };
                let value = match &attribute.value {
                    None => PropValue::Literal("true".into()), // <Link prefetch />
                    Some(JSXAttributeValue::StringLiteral(s)) => match s.value.as_str() {
                        // A lone surrogate can't be a static literal: read it at runtime instead.
                        Some(value) => PropValue::Literal(json_string(value)),
                        None => PropValue::Expression(self.text(s.span)),
                    },
                    Some(JSXAttributeValue::ExpressionContainer(container)) => match &container
                        .expression
                    {
                        JSXExpression::EmptyExpression(_) => PropValue::Expression(String::new()),
                        expression => literal(expression).map_or_else(
                            || {
                                PropValue::Expression(
                                    self.text(oxc_span::GetSpan::span(expression)),
                                )
                            },
                            PropValue::Literal,
                        ),
                    },
                    Some(other) => PropValue::Expression(self.text(oxc_span::GetSpan::span(other))),
                };
                Prop { name, value }
            }
        }
    }
}

/// The JSON of a literal expression, or `None` if the expression isn't a literal.
fn literal(expression: &JSXExpression<'_>) -> Option<String> {
    match expression {
        JSXExpression::StringLiteral(s) => s.value.as_str().map(json_string),
        JSXExpression::NumericLiteral(n) => json_number(n.value),
        JSXExpression::BooleanLiteral(b) => Some(b.value.to_string()),
        JSXExpression::NullLiteral(_) => Some("null".into()),
        JSXExpression::TemplateLiteral(t) if t.expressions.is_empty() => t
            .quasis
            .first()
            .and_then(|q| q.value.cooked?.as_str())
            .map(json_string),
        JSXExpression::UnaryExpression(u) if u.operator == UnaryOperator::UnaryNegation => {
            match &u.argument {
                Expression::NumericLiteral(n) => json_number(-n.value),
                _ => None,
            }
        }
        _ => None,
    }
}

/// A JSON number as `JSON.stringify` writes it, or `None` for values JSON can't hold.
fn json_number(n: f64) -> Option<String> {
    if !n.is_finite() {
        return None;
    }
    if n == 0.0 {
        return Some("0".into()); // JSON.stringify(-0) is "0"
    }
    Some(ryu_js::Buffer::new().format_finite(n).to_owned())
}

/// A JSON string literal (as `JSON.stringify` writes it).
fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if u32::from(c) < 0x20 => {
                use std::fmt::Write as _;
                let _ = write!(out, "\\u{:04x}", u32::from(c));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Whether a JSX child produces a child (React drops whitespace-only text that spans lines, and
/// empty `{}` / `{/* comments */}` containers).
fn is_child(child: &JSXChild<'_>) -> bool {
    match child {
        JSXChild::Text(text) => jsx_text(text.value.as_str()).is_some(),
        JSXChild::ExpressionContainer(c) => {
            !matches!(c.expression, JSXExpression::EmptyExpression(_))
        }
        JSXChild::Element(_) | JSXChild::Fragment(_) | JSXChild::Spread(_) => true,
    }
}

/// JSX text as React sees it: lines trimmed (except the start of the first and the end of the
/// last), whitespace-only lines dropped, the rest joined with single spaces. `None` if nothing is
/// left. This is the rule Babel, TypeScript and oxc's JSX transform apply.
#[must_use]
pub fn jsx_text(raw: &str) -> Option<String> {
    let lines: Vec<&str> = raw.split(['\n', '\r']).collect();
    let last = lines.len().saturating_sub(1);
    let mut parts = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let mut line: &str = line;
        if i > 0 {
            line = line.trim_start_matches([' ', '\t']);
        }
        if i < last {
            line = line.trim_end_matches([' ', '\t']);
        }
        if !line.is_empty() {
            parts.push(line);
        }
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" "))
    }
}

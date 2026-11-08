//! Template trees (ADR-0012, section 2): the server components a module defines, and for each, the
//! JSX in its template positions (ADR-0005, section 3) as a tree of static parts and holes.
//!
//! Spans are byte offsets into the module's source: the data function (step 5, C4) rewrites the
//! source by them, and the hole expressions run there, unchanged.

use oxc_ast::ast::{
    Argument, ArrowFunctionBody, ArrowFunctionExpression, BindingPattern, CallExpression,
    Declaration, ExportDefaultDeclarationKind, Expression, Function, FunctionBody,
    IdentifierReference, JSXAttributeItem, JSXAttributeName, JSXAttributeValue, JSXChild,
    JSXElement, JSXElementName, JSXMemberExpressionObject, LogicalOperator, Program, Statement,
    UnaryOperator,
};
use oxc_ast_visit::{Visit, walk};
use oxc_semantic::{Scoping, SymbolId};
use oxc_span::{GetSpan, Span};
use oxc_syntax::scope::ScopeFlags;

use crate::component::{decode_entities, jsx_text};

/// A source range: byte offsets (start, end).
pub type Range = (u32, u32);

/// A server component a module defines at its top level.
#[derive(Clone, Debug, PartialEq)]
pub struct Component {
    /// Its name (`ProductPage`), or `None` for an anonymous `export default`.
    pub name: Option<String>,
    /// Whether it's the module's default export.
    pub default_export: bool,
    /// Whether it's `async`.
    pub is_async: bool,
    /// The whole function.
    pub span: Range,
    /// Whether it calls a hook (`use`, or a `use…` name): React renders it then (ADR-0005,
    /// section 4), though its JSX still compiles.
    pub uses_hooks: bool,
    /// The JSX it returns, one template per `return` (or the arrow's expression body), in source
    /// order. A `return` of something else (`null`, `notFound()`) is a template too: a hole.
    pub returns: Vec<Template>,
    /// The props it only places (ADR-0005, section 6): a caller's JSX for them can be compiled as
    /// a slot, linked where they're placed.
    pub placed: Vec<Placed>,
}

/// A prop a component only places: destructured from its props (`{ children }`, no default),
/// and every reference to it is a `{children}` hole in its templates. A hole at a template's root
/// doesn't count: React gives that the component's key path (ADR-0008), and a hole passed on to
/// another component doesn't count either.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Placed {
    /// The prop's name.
    pub prop: String,
    /// The holes that place it, in source order.
    pub holes: Vec<Range>,
}

/// The JSX in one template position.
#[derive(Clone, Debug, PartialEq)]
pub struct Template {
    /// The returned expression: what the data function replaces with the template's record.
    pub span: Range,
    /// Its tree.
    pub root: Node,
}

/// A node of a template tree.
#[derive(Clone, Debug, PartialEq)]
pub enum Node {
    /// A host element compiled as template markup.
    Element(Element),
    /// Static text: JSX text after React's whitespace rules, or a string literal child.
    Text(String),
    /// A numeric literal child.
    Number(f64),
    /// A `null`, `true` or `false` literal child: nothing in HTML, the value in Flight.
    Literal(Literal),
    /// A fragment, `<>…</>`: its children in place in HTML, an array in Flight.
    Fragment(Vec<Node>),
    /// A value known only at runtime (`{x}`).
    Hole(Range),
    /// `test ? <A/> : <B/>`, `test && <A/>`, `test || <A/>`, `test ?? <A/>`.
    Conditional(Box<Conditional>),
    /// `array.map((item) => <Item/>)`.
    List(Box<List>),
    /// A component element: a pending call (ADR-0005, section 4), linked in step 5's C5.
    Component(Box<ComponentElement>),
    /// JSX kept as real JSX, evaluated at runtime into an element value (ADR-0012, section 2).
    ElementValue(ElementValue),
}

/// A literal child that renders nothing in HTML.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Literal {
    /// `null`
    Null,
    /// `true` / `false`
    Bool(bool),
}

/// A host element compiled as template markup.
#[derive(Clone, Debug, PartialEq)]
pub struct Element {
    /// The tag (`div`, `my-element`, `svg:rect`).
    pub tag: String,
    /// Its key.
    pub key: Option<Key>,
    /// Where the key is written: the number of props before it. After a spread, the JSX
    /// transform passes it inside the props object (`createElement`), so it's evaluated there.
    pub key_at: usize,
    /// Its props in source order, `key` excluded.
    pub props: Vec<Prop>,
    /// Its children, after JSX's whitespace rules.
    pub children: Vec<Node>,
    /// The whole element.
    pub span: Range,
}

/// An element's key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Key {
    /// A literal: the string React makes of it (`key={1}` is `"1"`).
    Static(String),
    /// An expression, converted with `String(…)` at runtime.
    Dynamic(Range),
}

/// A prop as written.
#[derive(Clone, Debug, PartialEq)]
pub enum Prop {
    /// A literal.
    Static {
        /// The name.
        name: String,
        /// The value.
        value: Value,
    },
    /// An expression.
    Dynamic {
        /// The name.
        name: String,
        /// The expression.
        value: Range,
    },
    /// `{...props}`.
    Spread(Range),
}

/// A literal prop value.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    /// A string.
    String(String),
    /// A finite number.
    Number(f64),
    /// `true` / `false` (a prop written without a value is `true`).
    Bool(bool),
    /// `null`
    Null,
}

impl Value {
    /// The value as `JSON.stringify` writes it.
    #[must_use]
    pub fn json(&self) -> String {
        match self {
            Self::String(s) => json_string(s),
            Self::Number(n) => json_number(*n).unwrap_or_else(|| "null".into()),
            Self::Bool(b) => b.to_string(),
            Self::Null => "null".into(),
        }
    }
}

/// Why JSX stays an element value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reason {
    /// It can hoist into `<head>` or emit a Flight hint (ADR-0011).
    Hoistable,
    /// It has React DOM rules of its own (ADR-0007).
    Special,
    /// It changes its descendants' context and has something runtime inside (ADR-0009).
    Context,
    /// It sets `dangerouslySetInnerHTML` or an explicit `children` prop.
    Content,
    /// It has a `ref`, which React's Flight server rejects unless it's `null`.
    Ref,
}

/// JSX kept as real JSX.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ElementValue {
    /// The JSX expression.
    pub span: Range,
    /// Why.
    pub reason: Reason,
}

/// Which conditional.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Condition {
    /// `test ? consequent : alternate`
    Ternary,
    /// `test && consequent`: when `test` is falsy, React renders its value.
    And,
    /// `test || consequent`: when `test` is truthy, React renders its value.
    Or,
    /// `test ?? consequent`: when `test` isn't `null` or `undefined`, React renders its value.
    Nullish,
}

/// A conditional in a template position.
#[derive(Clone, Debug, PartialEq)]
pub struct Conditional {
    /// Which one.
    pub condition: Condition,
    /// The test expression.
    pub test: Range,
    /// The template when the test passes (`?:`, `&&`) or fails (`||`, `??`).
    pub consequent: Node,
    /// For `?:`, the other branch.
    pub alternate: Option<Node>,
    /// The whole expression.
    pub span: Range,
}

/// `array.map(callback)` in a template position.
#[derive(Clone, Debug, PartialEq)]
pub struct List {
    /// The array expression (`items`, `items.filter(…)`).
    pub array: Range,
    /// The callback.
    pub callback: Range,
    /// The callback's templates, one per `return` (or its expression body).
    pub item: Vec<Template>,
    /// The whole call.
    pub span: Range,
}

/// A component element.
#[derive(Clone, Debug, PartialEq)]
pub struct ComponentElement {
    /// The local binding the tag refers to (`Price`, or `ui` in `<ui.Button>`).
    pub name: String,
    /// For `<ui.Button>`: `Button`.
    pub member: Option<String>,
    /// Whether `name` is a module-level binding (an import or a top-level declaration), so it
    /// refers to what the module graph says: only then can the element be linked.
    pub module_scope: bool,
    /// Its key.
    pub key: Option<Key>,
    /// Where the key is written: the number of props before it. After a spread, the JSX
    /// transform passes it inside the props object (`createElement`), so it's evaluated there.
    pub key_at: usize,
    /// Its props in source order: a prop whose value is JSX in a template position is a slot.
    pub props: Vec<ComponentProp>,
    /// Its children (the `children` slot).
    pub children: Vec<Node>,
    /// The whole element.
    pub span: Range,
}

/// A component element's prop.
#[derive(Clone, Debug, PartialEq)]
pub enum ComponentProp {
    /// A literal, an expression or a spread.
    Value(Prop),
    /// JSX: a slot the callee may place (ADR-0005, section 6).
    Slot {
        /// The name.
        name: String,
        /// Its template.
        template: Node,
    },
}

/// What a module's JSX can't be compiled from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Problem {
    /// Why.
    pub message: String,
    /// Where.
    pub span: Range,
}

/// The server components a module defines at its top level, with their templates, and what
/// can't be compiled. `scoping` is the module's (`oxc_semantic`): it tells which component tags
/// refer to module-level bindings.
#[must_use]
pub fn extract(program: &Program<'_>, scoping: &Scoping) -> (Vec<Component>, Vec<Problem>) {
    let mut extractor = Extractor {
        problems: Vec::new(),
        scoping,
        identifiers: Vec::new(),
    };
    let mut components = Vec::new();
    for statement in &program.body {
        extractor.statement(statement, &mut components);
    }
    // `export default Price;` or `export { Price as default };`, before or after its definition.
    for name in program.body.iter().filter_map(default_export_name) {
        for component in &mut components {
            if component.name.as_deref() == Some(name) {
                component.default_export = true;
            }
        }
    }
    (components, extractor.problems)
}

/// The holes of a template tree where a value is placed as it is: everywhere but inside a
/// component element (passed on as a prop) or an element value.
fn placed_holes(node: &Node, out: &mut Vec<Range>) {
    match node {
        Node::Hole(span) => out.push(*span),
        Node::Element(e) => e.children.iter().for_each(|c| placed_holes(c, out)),
        Node::Fragment(children) => children.iter().for_each(|c| placed_holes(c, out)),
        Node::Conditional(c) => {
            placed_holes(&c.consequent, out);
            if let Some(alternate) = &c.alternate {
                placed_holes(alternate, out);
            }
        }
        Node::List(l) => l.item.iter().for_each(|t| placed_holes(&t.root, out)),
        Node::Text(_)
        | Node::Number(_)
        | Node::Literal(_)
        | Node::Component(_)
        | Node::ElementValue(_) => {}
    }
}

/// The local name a statement exports as the default, when it names a binding.
fn default_export_name<'a>(statement: &'a Statement<'_>) -> Option<&'a str> {
    match statement {
        Statement::ExportDefaultDeclaration(export) => match &export.declaration {
            ExportDefaultDeclarationKind::Identifier(id) => Some(id.name.as_str()),
            _ => None,
        },
        Statement::ExportNamedDeclaration(export) => export
            .specifiers
            .iter()
            .find(|s| s.exported.name() == "default")
            .map(|s| s.local.name().as_str()),
        _ => None,
    }
}

struct Extractor<'s> {
    problems: Vec<Problem>,
    scoping: &'s Scoping,
    /// Holes that are a lone identifier, and the binding each refers to, in the component being
    /// extracted.
    identifiers: Vec<(Range, SymbolId)>,
}

/// A function that may be a component.
#[derive(Clone, Copy)]
enum Candidate<'b, 'a> {
    Function(&'b Function<'a>),
    Arrow(&'b ArrowFunctionExpression<'a>),
}

impl Extractor<'_> {
    fn statement(&mut self, statement: &Statement<'_>, out: &mut Vec<Component>) {
        match statement {
            Statement::FunctionDeclaration(function) => {
                self.named(
                    function.id.as_ref().map(|id| id.name.as_str()),
                    Candidate::Function(function),
                    false,
                    out,
                );
            }
            Statement::VariableDeclaration(declaration) => {
                for declarator in &declaration.declarations {
                    let (BindingPattern::BindingIdentifier(id), Some(init)) =
                        (&declarator.id, &declarator.init)
                    else {
                        continue;
                    };
                    if let Some(candidate) = candidate(init) {
                        self.named(Some(id.name.as_str()), candidate, false, out);
                    }
                }
            }
            // `export function F` / `export const F = …`
            Statement::ExportDeclaration(export) => match &export.declaration {
                Declaration::FunctionDeclaration(function) => {
                    self.named(
                        function.id.as_ref().map(|id| id.name.as_str()),
                        Candidate::Function(function),
                        false,
                        out,
                    );
                }
                Declaration::VariableDeclaration(declaration) => {
                    for declarator in &declaration.declarations {
                        let (BindingPattern::BindingIdentifier(id), Some(init)) =
                            (&declarator.id, &declarator.init)
                        else {
                            continue;
                        };
                        if let Some(candidate) = candidate(init) {
                            self.named(Some(id.name.as_str()), candidate, false, out);
                        }
                    }
                }
                _ => {}
            },
            Statement::ExportDefaultDeclaration(export) => match &export.declaration {
                ExportDefaultDeclarationKind::FunctionDeclaration(function) => {
                    self.named(
                        function.id.as_ref().map(|id| id.name.as_str()),
                        Candidate::Function(function),
                        true,
                        out,
                    );
                }
                kind => {
                    if let Some(candidate) = kind.as_expression().and_then(candidate) {
                        self.named(None, candidate, true, out);
                    }
                }
            },
            _ => {}
        }
    }

    /// A component: a default export, or a function bound to a capitalized name (as React
    /// decides what a component is).
    fn named(
        &mut self,
        name: Option<&str>,
        function: Candidate<'_, '_>,
        default_export: bool,
        out: &mut Vec<Component>,
    ) {
        let capitalized = name.is_some_and(|n| n.starts_with(|c: char| c.is_ascii_uppercase()));
        if !(default_export || capitalized) {
            return;
        }
        self.identifiers.clear();
        let (span, is_async, returns, uses_hooks) = match function {
            Candidate::Function(function) => {
                let Some(body) = &function.body else { return };
                (
                    function.span,
                    function.r#async,
                    self.body_returns(body),
                    calls_hooks(body),
                )
            }
            Candidate::Arrow(arrow) => (arrow.span, arrow.r#async, self.arrow_returns(arrow), {
                let mut hooks = Hooks(false);
                hooks.visit_arrow_function_expression(arrow);
                hooks.0
            }),
        };
        let params = match function {
            Candidate::Function(function) => function.params.items.first(),
            Candidate::Arrow(arrow) => arrow.params.items.first(),
        };
        let placed = params.map_or_else(Vec::new, |p| self.placed(&p.pattern, &returns));
        out.push(Component {
            name: name.map(str::to_owned),
            default_export,
            is_async,
            span: range(span),
            uses_hooks,
            returns,
            placed,
        });
    }

    /// The props a component destructures from `pattern` and only places in `returns`.
    fn placed(&self, pattern: &BindingPattern<'_>, returns: &[Template]) -> Vec<Placed> {
        let BindingPattern::ObjectPattern(object) = pattern else {
            return Vec::new();
        };
        let mut holes = Vec::new();
        for template in returns {
            if !matches!(template.root, Node::Hole(_)) {
                placed_holes(&template.root, &mut holes);
            }
        }
        object
            .properties
            .iter()
            .filter_map(|property| {
                let BindingPattern::BindingIdentifier(id) = &property.value else {
                    return None; // a default, or a nested pattern
                };
                let name = property.key.static_name()?;
                let prop = name.as_js_str().as_str()?.to_owned();
                let symbol = id.symbol_id.get()?;
                let placing: Vec<Range> = self
                    .identifiers
                    .iter()
                    .filter(|(span, s)| *s == symbol && holes.contains(span))
                    .map(|(span, _)| *span)
                    .collect();
                let references = self.scoping.get_resolved_reference_ids(symbol).len();
                let only_placed =
                    !property.computed && !placing.is_empty() && placing.len() == references;
                only_placed.then_some(Placed {
                    prop,
                    holes: placing,
                })
            })
            .collect()
    }

    /// An arrow's templates: its expression body, or its `return`s.
    fn arrow_returns(&mut self, arrow: &ArrowFunctionExpression<'_>) -> Vec<Template> {
        match &arrow.body {
            ArrowFunctionBody::FunctionBody(body) => self.body_returns(body),
            body => body
                .as_expression()
                .map(|expression| vec![self.template(expression)])
                .unwrap_or_default(),
        }
    }

    /// The templates of a function body's own `return`s (not those of nested functions).
    fn body_returns(&mut self, body: &FunctionBody<'_>) -> Vec<Template> {
        let mut returns = Returns {
            extractor: self,
            templates: Vec::new(),
        };
        returns.visit_function_body(body);
        returns.templates
    }

    fn template(&mut self, expression: &Expression<'_>) -> Template {
        Template {
            span: range(expression.span()),
            root: self.node(expression),
        }
    }

    /// An expression in a template position.
    fn node(&mut self, expression: &Expression<'_>) -> Node {
        let span = range(expression.span());
        match expression {
            Expression::ParenthesizedExpression(e) => self.node(&e.expression),
            Expression::TSAsExpression(e) => self.node(&e.expression),
            Expression::TSSatisfiesExpression(e) => self.node(&e.expression),
            Expression::TSNonNullExpression(e) => self.node(&e.expression),
            Expression::JSXElement(element) => self.element(element),
            Expression::JSXFragment(fragment) => Node::Fragment(self.children(&fragment.children)),
            Expression::StringLiteral(s) => match s.value.as_str() {
                Some(text) => Node::Text(text.to_owned()),
                None => Node::Hole(span), // a lone surrogate: read it at runtime
            },
            Expression::TemplateLiteral(t) if t.expressions.is_empty() => t
                .quasis
                .first()
                .and_then(|q| q.value.cooked.as_ref()?.as_str())
                .map_or(Node::Hole(span), |text| Node::Text(text.to_owned())),
            Expression::NumericLiteral(n) => Node::Number(n.value),
            Expression::UnaryExpression(u) if u.operator == UnaryOperator::UnaryNegation => {
                match &u.argument {
                    Expression::NumericLiteral(n) => Node::Number(-n.value),
                    _ => Node::Hole(span),
                }
            }
            Expression::NullLiteral(_) => Node::Literal(Literal::Null),
            Expression::BooleanLiteral(b) => Node::Literal(Literal::Bool(b.value)),
            Expression::ConditionalExpression(c) => {
                let (consequent, alternate) = (self.node(&c.consequent), self.node(&c.alternate));
                if is_runtime_only(&consequent) && is_runtime_only(&alternate) {
                    return Node::Hole(span); // no JSX in either branch: one value
                }
                Node::Conditional(Box::new(Conditional {
                    condition: Condition::Ternary,
                    test: range(c.test.span()),
                    consequent,
                    alternate: Some(alternate),
                    span,
                }))
            }
            Expression::LogicalExpression(l) => {
                let consequent = self.node(&l.right);
                if is_runtime_only(&consequent) {
                    return Node::Hole(span);
                }
                let condition = match l.operator {
                    LogicalOperator::And => Condition::And,
                    LogicalOperator::Or => Condition::Or,
                    LogicalOperator::Coalesce => Condition::Nullish,
                };
                Node::Conditional(Box::new(Conditional {
                    condition,
                    test: range(l.left.span()),
                    consequent,
                    alternate: None,
                    span,
                }))
            }
            Expression::CallExpression(call) => self.map_call(call).unwrap_or(Node::Hole(span)),
            Expression::Identifier(id) => {
                let symbol = id
                    .reference_id
                    .get()
                    .and_then(|reference| self.scoping.get_reference(reference).symbol_id());
                if let Some(symbol) = symbol {
                    self.identifiers.push((span, symbol));
                }
                Node::Hole(span)
            }
            _ => Node::Hole(span),
        }
    }

    /// `array.map(callback)` whose callback returns JSX.
    fn map_call(&mut self, call: &CallExpression<'_>) -> Option<Node> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return None;
        };
        if member.property.name != "map" || call.optional || call.arguments.len() != 1 {
            return None;
        }
        let argument = call.arguments.first()?;
        let item = match argument {
            Argument::ArrowFunctionExpression(arrow) => self.arrow_returns(arrow),
            Argument::FunctionExpression(function) => self.body_returns(function.body.as_ref()?),
            _ => return None,
        };
        if item.iter().all(|t| is_runtime_only(&t.root)) {
            return None; // `xs.map(String)`: one value
        }
        Some(Node::List(Box::new(List {
            array: range(member.object.span()),
            callback: range(argument.span()),
            item,
            span: range(call.span),
        })))
    }

    fn element(&mut self, element: &JSXElement<'_>) -> Node {
        let span = range(element.span);
        let opening = &element.opening_element;
        let tag = match &opening.name {
            JSXElementName::Identifier(id) => Some(id.name.as_str().to_owned()),
            JSXElementName::NamespacedName(n) => Some(format!(
                "{}:{}",
                n.namespace.name.as_str(),
                n.name.name.as_str()
            )),
            JSXElementName::IdentifierReference(_)
            | JSXElementName::MemberExpression(_)
            | JSXElementName::ThisExpression(_) => None,
        };
        let Some(tag) = tag else {
            return self.component(element);
        };
        let mut key = None;
        let mut key_at = 0;
        let mut props = Vec::new();
        for item in &opening.attributes {
            match Self::prop(item) {
                PropOrKey::Key(k) => {
                    key = Some(k);
                    key_at = props.len();
                }
                PropOrKey::Prop(p) => props.push(p),
            }
        }
        let children = self.children(&element.children);
        let element_value = |reason| Node::ElementValue(ElementValue { span, reason });
        if is_hoistable(&tag) {
            return element_value(Reason::Hoistable);
        }
        if is_special(&tag) {
            return element_value(Reason::Special);
        }
        let named = |name: &str| {
            props.iter().any(|p| matches!(p, Prop::Static { name: n, .. } | Prop::Dynamic { name: n, .. } if n == name))
        };
        if named("dangerouslySetInnerHTML") || named("children") {
            return element_value(Reason::Content);
        }
        if named("ref") {
            return element_value(Reason::Ref);
        }
        if changes_context(&tag) && children.iter().any(has_runtime_part) {
            return element_value(Reason::Context);
        }
        Node::Element(Element {
            tag,
            key,
            key_at,
            props,
            children,
            span,
        })
    }

    /// Whether `id` refers to a binding declared at module level (an import or a top-level
    /// declaration), not a parameter, a local or a global.
    fn module_scope(&self, id: &IdentifierReference<'_>) -> bool {
        id.reference_id
            .get()
            .and_then(|reference| self.scoping.get_reference(reference).symbol_id())
            .is_some_and(|symbol| {
                self.scoping.symbol_scope_id(symbol) == self.scoping.root_scope_id()
            })
    }

    fn component(&mut self, element: &JSXElement<'_>) -> Node {
        let opening = &element.opening_element;
        let (name, member, module_scope) = match &opening.name {
            JSXElementName::IdentifierReference(id) => {
                (id.name.as_str().to_owned(), None, self.module_scope(id))
            }
            JSXElementName::MemberExpression(m) => {
                let mut path = vec![m.property.name.as_str().to_owned()];
                let mut object = &m.object;
                let (name, module_scope) = loop {
                    match object {
                        JSXMemberExpressionObject::MemberExpression(inner) => {
                            path.push(inner.property.name.as_str().to_owned());
                            object = &inner.object;
                        }
                        JSXMemberExpressionObject::IdentifierReference(id) => {
                            break (id.name.as_str().to_owned(), self.module_scope(id));
                        }
                        JSXMemberExpressionObject::ThisExpression(t) => {
                            self.problem("`this` in a JSX tag", t.span);
                            return Node::Hole(range(element.span));
                        }
                    }
                };
                path.reverse();
                (name, Some(path.join(".")), module_scope)
            }
            _ => {
                self.problem("`this` as a JSX tag", opening.span);
                return Node::Hole(range(element.span));
            }
        };
        let mut key = None;
        let mut key_at = 0;
        let mut props = Vec::new();
        for item in &opening.attributes {
            if let JSXAttributeItem::Attribute(attribute) = item
                && let Some(JSXAttributeValue::ExpressionContainer(container)) = &attribute.value
                && let Some(expression) = container.expression.as_expression()
                && is_jsx(expression)
            {
                props.push(ComponentProp::Slot {
                    name: attribute_name(&attribute.name),
                    template: self.node(expression),
                });
                continue;
            }
            match Self::prop(item) {
                PropOrKey::Key(k) => {
                    key = Some(k);
                    key_at = props.len();
                }
                PropOrKey::Prop(p) => props.push(ComponentProp::Value(p)),
            }
        }
        Node::Component(Box::new(ComponentElement {
            name,
            member,
            module_scope,
            key,
            key_at,
            props,
            children: self.children(&element.children),
            span: range(element.span),
        }))
    }

    fn prop(item: &JSXAttributeItem<'_>) -> PropOrKey {
        let attribute = match item {
            JSXAttributeItem::SpreadAttribute(spread) => {
                return PropOrKey::Prop(Prop::Spread(range(spread.argument.span())));
            }
            JSXAttributeItem::Attribute(attribute) => attribute,
        };
        let name = attribute_name(&attribute.name);
        let value = match &attribute.value {
            None => Ok(Value::Bool(true)), // <input disabled />
            // A JSX attribute's string has its character references decoded (`&copy;`).
            Some(JSXAttributeValue::StringLiteral(s)) => s
                .value
                .as_str()
                .map(|v| Value::String(decode_entities(v).into_owned()))
                .ok_or(range(s.span)),
            // The parser rejects an empty `{}` prop value, so the expression is there.
            Some(JSXAttributeValue::ExpressionContainer(container)) => container
                .expression
                .as_expression()
                .map_or(Err(range(container.span)), |expression| {
                    literal_value(expression).ok_or(range(expression.span()))
                }),
            Some(other) => Err(range(other.span())),
        };
        if name == "key" {
            let expression = match &attribute.value {
                Some(JSXAttributeValue::ExpressionContainer(c)) => c.expression.as_expression(),
                _ => None,
            };
            let literal = match &attribute.value {
                Some(JSXAttributeValue::StringLiteral(s)) => {
                    s.value.as_str().map(|v| decode_entities(v).into_owned())
                }
                _ => expression.and_then(key_literal),
            };
            return PropOrKey::Key(match (literal, expression) {
                (Some(key), _) => Key::Static(key),
                (None, Some(expression)) => Key::Dynamic(range(expression.span())),
                (None, None) => Key::Dynamic(range(attribute.span)),
            });
        }
        PropOrKey::Prop(match value {
            Ok(value) => Prop::Static { name, value },
            Err(value) => Prop::Dynamic { name, value },
        })
    }

    fn children(&mut self, children: &[JSXChild<'_>]) -> Vec<Node> {
        let mut nodes = Vec::new();
        for child in children {
            match child {
                JSXChild::Text(text) => {
                    if let Some(text) = jsx_text(text.value.as_str()) {
                        nodes.push(Node::Text(text));
                    }
                }
                JSXChild::Element(element) => nodes.push(self.element(element)),
                JSXChild::Fragment(fragment) => {
                    nodes.push(Node::Fragment(self.children(&fragment.children)));
                }
                JSXChild::ExpressionContainer(container) => {
                    if let Some(expression) = container.expression.as_expression() {
                        nodes.push(self.node(expression));
                    }
                }
                JSXChild::Spread(spread) => {
                    // React's JSX transforms reject spread children.
                    self.problem("spread children (`{...children}`)", spread.span);
                    nodes.push(Node::Hole(range(spread.span)));
                }
            }
        }
        nodes
    }

    fn problem(&mut self, what: &str, span: Span) {
        self.problems.push(Problem {
            message: format!("can't compile {what}"),
            span: range(span),
        });
    }
}

enum PropOrKey {
    Key(Key),
    Prop(Prop),
}

fn candidate<'b, 'a>(expression: &'b Expression<'a>) -> Option<Candidate<'b, 'a>> {
    match expression {
        Expression::ArrowFunctionExpression(arrow) => Some(Candidate::Arrow(arrow)),
        Expression::FunctionExpression(function) => Some(Candidate::Function(function)),
        Expression::ParenthesizedExpression(e) => candidate(&e.expression),
        _ => None,
    }
}

fn attribute_name(name: &JSXAttributeName<'_>) -> String {
    match name {
        JSXAttributeName::Identifier(id) => id.name.as_str().to_owned(),
        JSXAttributeName::NamespacedName(n) => {
            format!("{}:{}", n.namespace.name.as_str(), n.name.name.as_str())
        }
    }
}

fn range(span: Span) -> Range {
    (span.start, span.end)
}

/// JSX (or a conditional, list or fragment of it) in a template position.
fn is_jsx(expression: &Expression<'_>) -> bool {
    match expression {
        Expression::JSXElement(_) | Expression::JSXFragment(_) => true,
        Expression::ParenthesizedExpression(e) => is_jsx(&e.expression),
        Expression::ConditionalExpression(c) => is_jsx(&c.consequent) || is_jsx(&c.alternate),
        Expression::LogicalExpression(l) => is_jsx(&l.right),
        _ => false,
    }
}

/// A node with no template in it: one value, a hole.
const fn is_runtime_only(node: &Node) -> bool {
    matches!(node, Node::Hole(_))
}

/// Whether anything in the node is known only at runtime.
fn has_runtime_part(node: &Node) -> bool {
    match node {
        Node::Text(_) | Node::Number(_) | Node::Literal(_) => false,
        Node::Element(e) => {
            e.props.iter().any(|p| !matches!(p, Prop::Static { .. }))
                || matches!(e.key, Some(Key::Dynamic(_)))
                || e.children.iter().any(has_runtime_part)
        }
        Node::Fragment(children) => children.iter().any(has_runtime_part),
        Node::Hole(_)
        | Node::Conditional(_)
        | Node::List(_)
        | Node::Component(_)
        | Node::ElementValue(_) => true,
    }
}

/// Elements that can hoist into `<head>` or emit a Flight hint (ADR-0011).
fn is_hoistable(tag: &str) -> bool {
    matches!(tag, "title" | "meta" | "link" | "script" | "style" | "img")
}

/// Elements with React DOM rules of their own (ADR-0007).
fn is_special(tag: &str) -> bool {
    matches!(
        tag,
        "input"
            | "select"
            | "option"
            | "textarea"
            | "button"
            | "form"
            | "object"
            | "pre"
            | "listing"
            | "menuitem"
    )
}

/// Elements that change their descendants' context (ADR-0009).
fn changes_context(tag: &str) -> bool {
    matches!(
        tag,
        "svg"
            | "noscript"
            | "picture"
            | "math"
            | "foreignObject"
            | "table"
            | "thead"
            | "tbody"
            | "tfoot"
            | "colgroup"
            | "tr"
    )
}

/// A literal expression's value, or `None` if it isn't one (or is a lone surrogate, which only
/// reading it at runtime gets right).
fn literal_value(expression: &Expression<'_>) -> Option<Value> {
    let string = |s: Option<&str>| s.map(|s| Value::String(s.to_owned()));
    match expression {
        Expression::StringLiteral(s) => string(s.value.as_str()),
        Expression::NumericLiteral(n) => Some(Value::Number(n.value)),
        Expression::BooleanLiteral(b) => Some(Value::Bool(b.value)),
        Expression::NullLiteral(_) => Some(Value::Null),
        Expression::TemplateLiteral(t) if t.expressions.is_empty() => string(
            t.quasis
                .first()
                .and_then(|q| q.value.cooked.as_ref()?.as_str()),
        ),
        Expression::UnaryExpression(u) if u.operator == UnaryOperator::UnaryNegation => {
            match &u.argument {
                Expression::NumericLiteral(n) => Some(Value::Number(-n.value)),
                _ => None,
            }
        }
        Expression::ParenthesizedExpression(e) => literal_value(&e.expression),
        _ => None,
    }
}

/// The string React makes of a literal key, `"" + key`, or `None` if it isn't a literal.
fn key_literal(expression: &Expression<'_>) -> Option<String> {
    match expression {
        Expression::StringLiteral(s) => s.value.as_str().map(str::to_owned),
        Expression::TemplateLiteral(t) if t.expressions.is_empty() => t
            .quasis
            .first()
            .and_then(|q| q.value.cooked.as_ref()?.as_str())
            .map(str::to_owned),
        Expression::NumericLiteral(n) => Some(js_number(n.value)),
        Expression::UnaryExpression(u) if u.operator == UnaryOperator::UnaryNegation => {
            match &u.argument {
                Expression::NumericLiteral(n) => Some(js_number(-n.value)),
                _ => None,
            }
        }
        Expression::BooleanLiteral(b) => Some(b.value.to_string()),
        Expression::NullLiteral(_) => Some("null".into()),
        Expression::ParenthesizedExpression(e) => key_literal(&e.expression),
        _ => None,
    }
}

/// JavaScript's `String(n)` for a finite literal (`-0` is `"0"`).
fn js_number(n: f64) -> String {
    if n == 0.0 {
        "0".into()
    } else {
        ryu_js::Buffer::new().format_finite(n).to_owned()
    }
}

fn json_number(n: f64) -> Option<String> {
    crate::component::json_number(n)
}

fn json_string(s: &str) -> String {
    crate::component::json_string(s)
}

/// A function body's own `return`s, as templates; nested functions and classes are skipped.
/// A bare `return;` is a hole: it renders `undefined`.
struct Returns<'e, 's> {
    extractor: &'e mut Extractor<'s>,
    templates: Vec<Template>,
}

impl<'a> Visit<'a> for Returns<'_, '_> {
    fn visit_return_statement(&mut self, statement: &oxc_ast::ast::ReturnStatement<'a>) {
        let template = match &statement.argument {
            Some(argument) => self.extractor.template(argument),
            None => Template {
                span: range(statement.span),
                root: Node::Hole(range(statement.span)),
            },
        };
        self.templates.push(template);
    }
    fn visit_function(&mut self, _: &Function<'a>, _: ScopeFlags) {}
    fn visit_arrow_function_expression(&mut self, _: &ArrowFunctionExpression<'a>) {}
    fn visit_class(&mut self, _: &oxc_ast::ast::Class<'a>) {}
}

/// Whether a function body calls a hook: `use(…)` or `useX(…)`.
fn calls_hooks(body: &FunctionBody<'_>) -> bool {
    let mut hooks = Hooks(false);
    hooks.visit_function_body(body);
    hooks.0
}

struct Hooks(bool);

impl<'a> Visit<'a> for Hooks {
    fn visit_call_expression(&mut self, call: &CallExpression<'a>) {
        let name = match &call.callee {
            Expression::Identifier(id) => Some(id.name.as_str()),
            Expression::StaticMemberExpression(m) => Some(m.property.name.as_str()),
            _ => None,
        };
        if name.is_some_and(is_hook_name) {
            self.0 = true;
        }
        walk::walk_call_expression(self, call);
    }
}

/// React's hook naming rule: `use`, or `use` followed by a capital letter or digit.
fn is_hook_name(name: &str) -> bool {
    name == "use"
        || name
            .strip_prefix("use")
            .and_then(|rest| rest.chars().next())
            .is_some_and(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
}

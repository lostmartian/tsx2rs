//! Lowering (ADR-0012, sections 1 and 3): a component's template trees become a template program
//! and its **record**, the values its data function writes, in the order the program reads them.
//!
//! Static parts are folded by the engine (`tsx2rs_engine::fold`), so every static byte is written
//! by the same code as at runtime (ARCHITECTURE.md invariant 5). Adjacent static parts merge into
//! one `Op::Static`, with React's `<!-- -->` separator between two text nodes; the separators next
//! to holes are the engine's, decided by each chunk's text edges.

use std::collections::HashMap;

use tsx2rs_engine::fold::{self, FoldError};
use tsx2rs_ir::attribute::{ElementRules, Prop as Class, classify, is_custom_element};
use tsx2rs_ir::input::{InputWriter, tag};
use tsx2rs_ir::{ChunkId, FormatError, Op, OpIndex, Program};

use crate::graph::{Module, ModuleGraph, ModuleId};
use crate::template::{
    Component, ComponentElement, ComponentProp, Condition, Element, Key, Literal, Node, Prop,
    Range, Template, Value,
};
use crate::usage::{self, Export};

/// A component lowered: its program and its record.
#[derive(Debug)]
pub struct Lowered {
    /// The template program.
    pub program: Program,
    /// What the data function writes, in program order.
    pub record: Vec<Slot>,
}

/// One part of a record.
#[derive(Clone, Debug, PartialEq)]
pub enum Slot {
    /// One value an op reads, computed by the expression at `span`.
    Value {
        /// Which op reads it, and so how it's encoded.
        kind: ValueKind,
        /// The expression.
        span: Range,
    },
    /// An element's props object, built as JSX builds it (spreads included), read by
    /// `Op::Attributes`.
    Props {
        /// The props, in source order.
        props: Vec<Prop>,
        /// The element.
        span: Range,
    },
    /// A condition (`If`), then the record of the branch it chose.
    Branch {
        /// What the condition tests.
        test: Test,
        /// The tested expression.
        span: Range,
        /// The record when the condition holds.
        then: Vec<Self>,
        /// The record otherwise.
        otherwise: Vec<Self>,
    },
    /// A list (`Loop`): the item count, then each item's record.
    List {
        /// The array expression.
        array: Range,
        /// The callback.
        callback: Range,
        /// The item's record, as the `Returns` of the callback.
        item: Box<Self>,
    },
    /// A linked child component (ADR-0005, section 6): its template is inlined, and its data
    /// comes from its own data function, called with this pending call's props once the
    /// caller's record is complete (ADR-0005, section 4).
    Call {
        /// The module that defines the callee.
        module: ModuleId,
        /// The callee's index in that module's components.
        component: usize,
        /// The component element's key: the runtime's key path (ADR-0008).
        key: Option<Key>,
        /// Where the key is written among the props (`ComponentElement::key_at`).
        key_at: usize,
        /// Its props, in source order, as JSX builds them (no slots: those aren't linked yet).
        props: Vec<ComponentProp>,
        /// Its children (text, literals and holes only), the `children` prop.
        children: Vec<Node>,
        /// The element.
        span: Range,
        /// The callee's own record, built by its data function.
        record: Vec<Self>,
    },
    /// Which `return` ran, as a chain of conditions, then that return's record. One return is
    /// just its record.
    Returns(Vec<(Range, Vec<Self>)>),
    /// The root element's Flight key, computed by the runtime from the key path (ADR-0008).
    RootKey,
    /// Whether React's Flight wraps the root element in an array (ADR-0008), computed by the
    /// runtime; read once before the element and once after it.
    RootWrap,
}

/// How a value is encoded: by the op that reads it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValueKind {
    /// `Op::Prop`: a primitive.
    Prop,
    /// `Op::Style`: a style object, `null` or `undefined`.
    Style,
    /// `Op::Value`: any value, Flight only (an event handler, a prop React doesn't write).
    FlightOnly,
    /// `Op::Child`: a React node.
    Child,
    /// `Op::Value`: an element's key, `String(key)`.
    Key,
}

/// What a branch's condition tests.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Test {
    /// JavaScript truthiness.
    Truthy,
    /// Neither `null` nor `undefined`.
    NotNullish,
    /// Always true: a list item whose record would otherwise be empty reads one condition, so
    /// every loop iteration reads input (`Program::new`'s rule).
    Present,
}

/// Why a component can't be lowered.
#[derive(Debug, thiserror::Error)]
pub enum LowerError {
    /// Folding a static part failed: React would reject it at runtime (a void element with
    /// children, a `style` string), or an element isn't template markup.
    #[error("can't fold static markup at {span:?}: {error}")]
    Fold {
        /// What failed.
        error: FoldError,
        /// Where.
        span: Range,
    },
    /// The program failed validation (a bug in lowering).
    #[error("invalid program: {0}")]
    Program(#[from] FormatError),
    /// The component has no template.
    #[error("the component returns nothing")]
    NoReturn,
    /// [`linked`] was given a component that doesn't exist.
    #[error("no component {index} in module {module:?}")]
    NoComponent {
        /// The module.
        module: ModuleId,
        /// The index asked for.
        index: usize,
    },
}

/// Lowers a component's templates into one program, on its own: every component element is a
/// `Child` hole, rendered at runtime.
///
/// # Errors
///
/// [`LowerError`] when a static part can't be folded or the component has no `return`.
pub fn component(component: &Component) -> Result<Lowered, LowerError> {
    lower(Lowering::default(), component)
}

/// Lowers the `index`th component of `module` with its child components linked (ADR-0012,
/// section 5): a child that the module graph resolves to a compiled server component is inlined
/// into the program. A child stays a `Child` hole, rendered at runtime, when it's:
/// - recursive (`Call` comes later), async, or calls a hook (React renders it, ADR-0005);
/// - given JSX props or JSX children (slots come later);
/// - named by anything but a module-level binding, or not a component the graph knows;
/// - impossible to lower itself.
///
/// # Errors
///
/// [`LowerError`] when a static part of the component itself can't be folded, it has no
/// `return`, or there's no such component.
pub fn linked(graph: &ModuleGraph, module: ModuleId, index: usize) -> Result<Lowered, LowerError> {
    let component = graph
        .modules
        .get(module.0)
        .and_then(|m| m.components.get(index))
        .ok_or(LowerError::NoComponent { module, index })?;
    let lowering = Lowering {
        link: Some(Link {
            graph,
            from: module,
            stack: vec![(module, index)],
        }),
        ..Lowering::default()
    };
    lower(lowering, component)
}

fn lower(mut lowering: Lowering<'_>, component: &Component) -> Result<Lowered, LowerError> {
    let record = lowering.returns(&component.returns, true)?;
    let record = returns_record(record);
    lowering.flush();
    let program = Program::new(lowering.chunks, lowering.ops)?;
    Ok(Lowered { program, record })
}

/// A component's record: one return's record is just that record.
fn returns_record(record: Slot) -> Vec<Slot> {
    match record {
        Slot::Returns(mut returns) if returns.len() == 1 => returns.remove(0).1,
        record => vec![record],
    }
}

/// Linking state: the graph, the module whose source is being lowered, and the components being
/// inlined (to find recursion).
struct Link<'g> {
    graph: &'g ModuleGraph,
    from: ModuleId,
    stack: Vec<(ModuleId, usize)>,
}

impl<'g> Link<'g> {
    /// The compiled server component `element` links to, if it can be inlined.
    fn callee(&self, element: &ComponentElement) -> Option<(ModuleId, usize, &'g Component)> {
        let simple = |node: &Node| {
            matches!(
                node,
                Node::Text(_) | Node::Number(_) | Node::Literal(_) | Node::Hole(_)
            )
        };
        if !element.module_scope
            || element
                .props
                .iter()
                .any(|p| matches!(p, ComponentProp::Slot { .. }))
            || !element.children.iter().all(simple)
        {
            return None;
        }
        let tag = (&element.name, element.member.as_deref());
        let (module, index) = match usage::component(self.graph, self.from, tag.0, tag.1) {
            usage::Component::Server { module, export } => {
                (module, exported(&self.graph.modules[module.0], &export)?)
            }
            usage::Component::Local if tag.1.is_none() => {
                let components = &self.graph.modules[self.from.0].components;
                let index = components
                    .iter()
                    .position(|c| c.name.as_deref() == Some(tag.0.as_str()))?;
                (self.from, index)
            }
            _ => return None,
        };
        let component = &self.graph.modules[module.0].components[index];
        let linkable =
            !component.is_async && !component.uses_hooks && !self.stack.contains(&(module, index));
        linkable.then_some((module, index, component))
    }
}

/// The index of the component `module` exports as `export`.
fn exported(module: &Module, export: &Export) -> Option<usize> {
    module.components.iter().position(|c| match export {
        Export::Default => c.default_export,
        Export::Named(name) => {
            c.name.as_deref() == Some(name.as_str()) && module.exports.local.contains(name)
        }
    })
}

/// Where lowering is, to undo an inlining that failed.
struct Mark {
    ops: usize,
    chunks: usize,
    pending: Pending,
}

/// Static output not yet written as an op.
#[derive(Clone, Default)]
struct Pending {
    html: Vec<u8>,
    flight: Vec<u8>,
    text_start: bool,
    text_end: bool,
}

#[derive(Default)]
struct Lowering<'g> {
    chunks: Vec<Box<[u8]>>,
    interned: HashMap<Vec<u8>, u32>,
    ops: Vec<Op>,
    pending: Pending,
    /// `None` when lowering a component on its own.
    link: Option<Link<'g>>,
}

impl Lowering<'_> {
    fn mark(&self) -> Mark {
        Mark {
            ops: self.ops.len(),
            chunks: self.chunks.len(),
            pending: self.pending.clone(),
        }
    }

    fn undo(&mut self, mark: Mark) {
        self.ops.truncate(mark.ops);
        self.chunks.truncate(mark.chunks);
        self.interned.retain(|_, id| (*id as usize) < mark.chunks);
        self.pending = mark.pending;
    }

    /// A component element: inlined when it links to a compiled component, else a `Child` hole.
    fn component_element(&mut self, element: &ComponentElement, record: &mut Vec<Slot>) {
        let Some((module, index, callee)) =
            self.link.as_ref().and_then(|link| link.callee(element))
        else {
            self.child(element.span, record);
            return;
        };
        let mark = self.mark();
        let from = self.enter(module, index);
        let inlined = self.returns(&callee.returns, true);
        self.leave(from);
        if let Ok(inlined) = inlined {
            record.push(Slot::Call {
                module,
                component: index,
                key: element.key.clone(),
                key_at: element.key_at,
                props: element.props.clone(),
                children: element.children.clone(),
                span: element.span,
                record: returns_record(inlined),
            });
        } else {
            // The callee can't be compiled (its own lowering reports why): React renders it.
            self.undo(mark);
            self.child(element.span, record);
        }
    }

    /// Starts lowering a callee's source; returns the caller's module.
    fn enter(&mut self, module: ModuleId, index: usize) -> ModuleId {
        match self.link.as_mut() {
            Some(link) => {
                link.stack.push((module, index));
                std::mem::replace(&mut link.from, module)
            }
            None => module,
        }
    }

    fn leave(&mut self, from: ModuleId) {
        if let Some(link) = self.link.as_mut() {
            link.stack.pop();
            link.from = from;
        }
    }

    fn chunk(&mut self, bytes: &[u8]) -> ChunkId {
        if let Some(&id) = self.interned.get(bytes) {
            return ChunkId(id);
        }
        let id = u32::try_from(self.chunks.len()).unwrap_or(u32::MAX);
        self.chunks.push(bytes.into());
        self.interned.insert(bytes.to_vec(), id);
        ChunkId(id)
    }

    /// Static output: HTML (with whether it starts and ends with text) and Flight.
    fn emit(&mut self, html: &[u8], flight: &[u8], text_start: bool, text_end: bool) {
        let p = &mut self.pending;
        if !html.is_empty() {
            if p.html.is_empty() {
                p.text_start = text_start;
            } else if p.text_end && text_start {
                p.html.extend_from_slice(b"<!-- -->"); // React's separator between text nodes
            }
            p.html.extend_from_slice(html);
            p.text_end = text_end;
        }
        p.flight.extend_from_slice(flight);
    }

    /// Markup or Flight structure, never text.
    fn structure(&mut self, html: &[u8], flight: &[u8]) {
        self.emit(html, flight, false, false);
    }

    /// Writes the pending static output as one `Op::Static`.
    fn flush(&mut self) {
        let pending = std::mem::take(&mut self.pending);
        if pending.html.is_empty() && pending.flight.is_empty() {
            return;
        }
        let (html, flight) = (self.chunk(&pending.html), self.chunk(&pending.flight));
        self.ops.push(Op::Static {
            html,
            flight,
            text_start: pending.text_start,
            text_end: pending.text_end,
        });
    }

    /// An op that reads input, after the pending static output.
    fn op(&mut self, op: Op) {
        self.flush();
        self.ops.push(op);
    }

    /// An op whose target is patched later: pushed after the pending static output, and its
    /// index returned.
    fn placeholder(&mut self, op: Op) -> usize {
        self.flush();
        self.ops.push(op);
        self.ops.len() - 1
    }

    fn here(&self) -> OpIndex {
        OpIndex(u32::try_from(self.ops.len()).unwrap_or(u32::MAX))
    }

    /// A component's (`roots`) or a list callback's returns: one, or a chain of conditions
    /// choosing one.
    fn returns(&mut self, templates: &[Template], roots: bool) -> Result<Slot, LowerError> {
        let Some((last, rest)) = templates.split_last() else {
            return Err(LowerError::NoReturn);
        };
        let mut returns = Vec::new();
        let mut jumps = Vec::new();
        for template in rest {
            let test = self.placeholder(Op::If { else_: OpIndex(0) });
            let record = self.template(&template.root, roots)?;
            self.flush();
            jumps.push(self.here());
            self.ops.push(Op::Jump { to: OpIndex(0) });
            let otherwise = self.here();
            self.ops[test] = Op::If { else_: otherwise };
            returns.push((template.span, record));
        }
        let record = self.template(&last.root, roots)?;
        self.flush();
        let end = self.here();
        for jump in jumps {
            self.ops[jump.0 as usize] = Op::Jump { to: end };
        }
        returns.push((last.span, record));
        Ok(Slot::Returns(returns))
    }

    /// A template. A component's root element gets the runtime's key path and wrap (ADR-0008);
    /// a list callback isn't a component, so its elements don't.
    fn template(&mut self, node: &Node, root: bool) -> Result<Vec<Slot>, LowerError> {
        let mut record = Vec::new();
        if let (Node::Element(element), true) = (node, root) {
            self.wrap(&mut record, b"[");
            self.element(element, &mut record, true)?;
            self.wrap(&mut record, b"]");
        } else {
            self.node(node, &mut record)?;
        }
        Ok(record)
    }

    /// `If(wrap) [flight]`: half of React's implicit-slot array around a root element.
    fn wrap(&mut self, record: &mut Vec<Slot>, flight: &[u8]) {
        let test = self.placeholder(Op::If { else_: OpIndex(0) });
        self.structure(b"", flight);
        self.flush();
        let end = self.here();
        self.ops[test] = Op::If { else_: end };
        record.push(Slot::RootWrap);
    }

    fn node(&mut self, node: &Node, record: &mut Vec<Slot>) -> Result<(), LowerError> {
        match node {
            Node::Element(element) => self.element(element, record, false),
            Node::Text(text) => {
                let mut w = InputWriter::new();
                w.tag(tag::STRING).string(text);
                self.fold_node(&w.finish(), (0, 0))
            }
            Node::Number(n) => {
                let mut w = InputWriter::new();
                w.tag(tag::NUMBER).number(*n);
                self.fold_node(&w.finish(), (0, 0))
            }
            Node::Literal(literal) => {
                let mut w = InputWriter::new();
                w.tag(match literal {
                    Literal::Null => tag::NULL,
                    Literal::Bool(true) => tag::TRUE,
                    Literal::Bool(false) => tag::FALSE,
                });
                self.fold_node(&w.finish(), (0, 0))
            }
            Node::Fragment(children) => self.children_value(children, record),
            Node::Hole(span) => {
                self.child(*span, record);
                Ok(())
            }
            Node::Component(element) => {
                self.component_element(element, record);
                Ok(())
            }
            Node::ElementValue(value) => {
                self.child(value.span, record);
                Ok(())
            }
            Node::Conditional(conditional) => self.conditional(conditional, record),
            Node::List(list) => self.list(list, record),
        }
    }

    /// A conditional: an `If` between its template and the value React renders otherwise.
    fn conditional(
        &mut self,
        conditional: &crate::template::Conditional,
        record: &mut Vec<Slot>,
    ) -> Result<(), LowerError> {
        let test = conditional.test;
        let (kind, then, otherwise) = match conditional.condition {
            // test ? consequent : alternate
            Condition::Ternary => (
                Test::Truthy,
                Branch::Node(&conditional.consequent),
                conditional
                    .alternate
                    .as_ref()
                    .map_or(Branch::Value(test), Branch::Node),
            ),
            // test && consequent: the value of `test` when it's falsy
            Condition::And => (
                Test::Truthy,
                Branch::Node(&conditional.consequent),
                Branch::Value(test),
            ),
            // test || consequent: the value of `test` when it's truthy
            Condition::Or => (
                Test::Truthy,
                Branch::Value(test),
                Branch::Node(&conditional.consequent),
            ),
            // test ?? consequent: the value of `test` unless it's nullish
            Condition::Nullish => (
                Test::NotNullish,
                Branch::Value(test),
                Branch::Node(&conditional.consequent),
            ),
        };
        self.branch(kind, test, then, otherwise, record)
    }

    /// `xs.map(…)`: a `Loop` over the callback's returns.
    fn list(
        &mut self,
        list: &crate::template::List,
        record: &mut Vec<Slot>,
    ) -> Result<(), LowerError> {
        // `xs.map(…)` is an array: `[item,item]` in Flight.
        self.structure(b"", b"[");
        let start = self.placeholder(Op::Loop {
            end: OpIndex(0),
            separator: None,
        });
        // Every iteration must read input (`Program::new`'s rule, which bounds a loop
        // by the input's length): a static item reads an always-true condition.
        let present = !list.item.iter().any(|t| reads_input(&t.root)) && list.item.len() == 1;
        let condition = self.here();
        if present {
            self.ops.push(Op::If { else_: OpIndex(0) });
        }
        let item = self.returns(&list.item, false)?;
        self.flush();
        let end = self.here();
        if present {
            self.ops[condition.0 as usize] = Op::If { else_: end };
        }
        self.ops.push(Op::EndLoop);
        let separator = self.chunk(b",");
        self.ops[start] = Op::Loop {
            end,
            separator: Some(separator),
        };
        self.structure(b"", b"]");
        let item = if present {
            Slot::Branch {
                test: Test::Present,
                span: list.callback,
                then: vec![item],
                otherwise: Vec::new(),
            }
        } else {
            item
        };
        record.push(Slot::List {
            array: list.array,
            callback: list.callback,
            item: Box::new(item),
        });
        Ok(())
    }

    fn child(&mut self, span: Range, record: &mut Vec<Slot>) {
        self.op(Op::Child);
        record.push(Slot::Value {
            kind: ValueKind::Child,
            span,
        });
    }

    /// `If` choosing between two branches.
    fn branch(
        &mut self,
        test: Test,
        span: Range,
        then: Branch<'_>,
        otherwise: Branch<'_>,
        record: &mut Vec<Slot>,
    ) -> Result<(), LowerError> {
        let at = self.placeholder(Op::If { else_: OpIndex(0) });
        let mut then_record = Vec::new();
        self.branch_body(then, &mut then_record)?;
        self.flush();
        let jump = self.here();
        self.ops.push(Op::Jump { to: OpIndex(0) });
        let else_ = self.here();
        let mut otherwise_record = Vec::new();
        self.branch_body(otherwise, &mut otherwise_record)?;
        self.flush();
        let end = self.here();
        self.ops[at] = Op::If { else_ };
        self.ops[jump.0 as usize] = Op::Jump { to: end };
        record.push(Slot::Branch {
            test,
            span,
            then: then_record,
            otherwise: otherwise_record,
        });
        Ok(())
    }

    fn branch_body(
        &mut self,
        branch: Branch<'_>,
        record: &mut Vec<Slot>,
    ) -> Result<(), LowerError> {
        match branch {
            Branch::Node(node) => self.node(node, record),
            Branch::Value(span) => {
                self.child(span, record);
                Ok(())
            }
        }
    }

    fn fold_node(&mut self, input: &[u8], span: Range) -> Result<(), LowerError> {
        let folded = fold::node(input).map_err(|error| LowerError::Fold { error, span })?;
        self.emit(
            &folded.html,
            &folded.flight,
            folded.text_start,
            folded.text_end,
        );
        Ok(())
    }

    /// Children as a node value: nothing for none (`"$undefined"` in Flight for a fragment), the
    /// child for one, an array for several, as JSX builds `children`.
    fn children_value(
        &mut self,
        children: &[Node],
        record: &mut Vec<Slot>,
    ) -> Result<(), LowerError> {
        match children {
            [] => self.structure(b"", br#""$undefined""#),
            [only] => self.node(only, record)?,
            children => {
                self.structure(b"", b"[");
                for (i, child) in children.iter().enumerate() {
                    if i > 0 {
                        self.structure(b"", b",");
                    }
                    self.node(child, record)?;
                }
                self.structure(b"", b"]");
            }
        }
        Ok(())
    }

    /// A host element as template markup: its edges and static props folded, its dynamic props
    /// and children lowered in source order.
    fn element(
        &mut self,
        e: &Element,
        record: &mut Vec<Slot>,
        root: bool,
    ) -> Result<(), LowerError> {
        let fold_error = |error| LowerError::Fold {
            error,
            span: e.span,
        };
        let tag = fold::tag(&e.tag).map_err(fold_error)?;
        if tag.void && !e.children.is_empty() {
            // React throws: "… is a self-closing tag and must neither have children …".
            return Err(fold_error(FoldError::NotGeneric));
        }
        let rules = if e.tag == "a" {
            ElementRules::Anchor
        } else if is_custom_element(&e.tag) {
            ElementRules::Custom
        } else {
            ElementRules::Generic
        };
        self.structure(&tag.html_open, &tag.flight_open);
        match (&e.key, root) {
            (_, true) => {
                self.op(Op::Value);
                record.push(Slot::RootKey);
            }
            (Some(Key::Dynamic(span)), false) => {
                self.op(Op::Value);
                record.push(Slot::Value {
                    kind: ValueKind::Key,
                    span: *span,
                });
            }
            (Some(Key::Static(key)), false) => self.structure(b"", &fold::key(Some(key))),
            (None, false) => self.structure(b"", &fold::key(None)),
        }
        self.structure(b"", b",{");
        let has_children = !e.children.is_empty();
        let wrote_props = if e.props.iter().any(|p| matches!(p, Prop::Spread(_))) {
            // A spread: the whole props object, as JSX builds it, classified at runtime.
            self.op(Op::Attributes {
                rules,
                comma_after: has_children,
            });
            record.push(Slot::Props {
                props: e.props.clone(),
                span: e.span,
            });
            false // the op writes its own comma
        } else {
            self.props(&e.props, rules, record, e.span)?
        };
        if tag.void {
            self.structure(b"/>", b"}]");
            return Ok(());
        }
        self.structure(b">", b"");
        if has_children {
            if wrote_props {
                self.structure(b"", b",");
            }
            self.structure(b"", br#""children":"#);
            self.children_value(&e.children, record)?;
        }
        self.structure(&tag.html_close, b"}]");
        Ok(())
    }

    /// Props without a spread: runs of static props folded, dynamic ones as ops. Returns whether
    /// any Flight pair was written.
    fn props(
        &mut self,
        props: &[Prop],
        rules: ElementRules,
        record: &mut Vec<Slot>,
        span: Range,
    ) -> Result<bool, LowerError> {
        let mut wrote = false;
        let mut i = 0;
        while i < props.len() {
            if wrote {
                self.structure(b"", b",");
            }
            match &props[i] {
                Prop::Static { .. } => {
                    let run: Vec<&Prop> = props[i..]
                        .iter()
                        .take_while(|p| matches!(p, Prop::Static { .. }))
                        .collect();
                    let mut w = InputWriter::new();
                    w.count(run.len());
                    for prop in &run {
                        if let Prop::Static { name, value } = prop {
                            w.string(name);
                            write_value(&mut w, value);
                        }
                    }
                    let folded = fold::props(rules, &w.finish())
                        .map_err(|error| LowerError::Fold { error, span })?;
                    self.structure(&folded.html, &folded.flight);
                    i += run.len();
                }
                Prop::Dynamic { name, value } => {
                    self.structure(b"", &fold::prop_key(name));
                    let (op, kind) = match classify(name, rules) {
                        Class::Attribute { name, kind } => (
                            Op::Prop {
                                name: self.chunk(name.as_bytes()),
                                kind,
                            },
                            ValueKind::Prop,
                        ),
                        Class::Style => (Op::Style, ValueKind::Style),
                        // Never written to HTML: Flight only (`ref` and content props make the
                        // element an element value before lowering).
                        Class::Ignored | Class::Ref | Class::Content => {
                            (Op::Value, ValueKind::FlightOnly)
                        }
                    };
                    self.op(op);
                    record.push(Slot::Value { kind, span: *value });
                    i += 1;
                }
                // The caller lowers an element with a spread through `Op::Attributes`.
                Prop::Spread(_) => {
                    i += 1;
                    continue;
                }
            }
            wrote = true;
        }
        Ok(wrote)
    }
}

/// A branch of an `If`.
#[derive(Clone, Copy)]
enum Branch<'n> {
    Node(&'n Node),
    /// The tested expression's own value, rendered as a child.
    Value(Range),
}

fn write_value(w: &mut InputWriter, value: &Value) {
    match value {
        Value::String(s) => {
            w.tag(tag::STRING).string(s);
        }
        Value::Number(n) => {
            w.tag(tag::NUMBER).number(*n);
        }
        Value::Bool(b) => {
            w.tag(if *b { tag::TRUE } else { tag::FALSE });
        }
        Value::Null => {
            w.tag(tag::NULL);
        }
    }
}

/// Whether a template reads input at runtime: anything but static markup and literals.
fn reads_input(node: &Node) -> bool {
    match node {
        Node::Text(_) | Node::Number(_) | Node::Literal(_) => false,
        Node::Element(e) => {
            e.props.iter().any(|p| !matches!(p, Prop::Static { .. }))
                || matches!(e.key, Some(Key::Dynamic(_)))
                || e.children.iter().any(reads_input)
        }
        Node::Fragment(children) => children.iter().any(reads_input),
        Node::Hole(_)
        | Node::Conditional(_)
        | Node::List(_)
        | Node::Component(_)
        | Node::ElementValue(_) => true,
    }
}

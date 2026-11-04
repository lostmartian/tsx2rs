//! Data functions (ADR-0012, section 4): a component's own source, rewritten so that each
//! returned template is replaced by the JavaScript that builds its **record**, the values its
//! program reads (`lower::Slot`). Everything else stays byte for byte, so evaluation order,
//! closures and module scope are the component's own.
//!
//! The record's shape is the contract with the encoder (step 6):
//! - a value is the expression's value; a key is `null` or `"" + key`, as React makes it;
//! - a props object (`Op::Attributes`) is built as JSX builds it, spreads included;
//! - a branch is `[taken, [record]]`, its test evaluated once (a branch that renders the tested
//!   value reads the same evaluation);
//! - a list is the component's own `.map` call, its callback returning item records;
//! - several returns are `[index, record]`; one is just its record;
//! - a linked child component is a pending call, `[config, key]` as passed to React's `jsx`: the
//!   runtime calls the child's own data function with its props after this record is complete,
//!   as React runs children after their parent (ADR-0013);
//! - the root's key path and wrap (ADR-0008) aren't in it: the runtime computes them.

use std::fmt::Write as _;

use crate::lower::{Slot, Test, ValueKind};
use crate::template::{Component, ComponentProp, Key, Literal, Node, Prop, Range};

/// The name of a component's data function.
#[must_use]
pub fn name(component: &Component) -> String {
    format!(
        "__tsx2rs_data_{}",
        component.name.as_deref().unwrap_or("default")
    )
}

/// The data functions for a module's components (each with its lowered record), as JavaScript
/// to append to the module's own source: they share its imports and module scope.
#[must_use]
pub fn functions(source: &str, components: &[(&Component, &[Slot])]) -> String {
    let mut out = String::from(
        "\n// tsx2rs data functions (generated): each returns its template's record.\n\
         const __tsx2rs_key = (key) => (key === undefined ? null : \"\" + key);\n",
    );
    for (component, record) in components {
        let mut writer = Writer { source, depth: 0 };
        let returns = match record {
            [Slot::Returns(returns)] => returns.clone(),
            record => {
                // One return: the record of the component's only template.
                let span = component.returns.first().map_or(component.span, |t| t.span);
                vec![(span, record.to_vec())]
            }
        };
        let replacements = returns_replacements(&mut writer, &returns);
        let function = writer.rewrite(component.span, &replacements);
        let _ = writeln!(out, "export const {} = {function};", name(component));
    }
    out
}

fn returns_replacements(
    writer: &mut Writer<'_>,
    returns: &[(Range, Vec<Slot>)],
) -> Vec<(Range, String)> {
    let several = returns.len() > 1;
    returns
        .iter()
        .enumerate()
        .map(|(i, (span, record))| {
            let record = writer.record(record, None);
            let text = if several {
                format!("[{i}, {record}]")
            } else {
                record
            };
            (*span, text)
        })
        .collect()
}

struct Writer<'s> {
    source: &'s str,
    /// Nesting of branches, for unique test variables.
    depth: usize,
}

impl Writer<'_> {
    fn text(&self, span: Range) -> &str {
        self.source
            .get(span.0 as usize..span.1 as usize)
            .unwrap_or_default()
    }

    /// `span`'s source, with each replacement (sorted, not overlapping, inside it) written in.
    fn rewrite(&self, span: Range, replacements: &[(Range, String)]) -> String {
        let mut replacements = replacements.to_vec();
        replacements.sort_by_key(|(r, _)| r.0);
        let mut out = String::new();
        let mut at = span.0;
        for ((start, end), text) in replacements {
            out.push_str(self.text((at, start)));
            out.push_str(&text);
            at = end;
        }
        out.push_str(self.text((at, span.1)));
        out
    }

    /// A record: a JavaScript array of its slots' values. `tested` is the variable holding the
    /// enclosing branch's test value, with that test's span.
    fn record(&mut self, slots: &[Slot], tested: Option<(Range, &str)>) -> String {
        let values: Vec<String> = slots
            .iter()
            .filter_map(|slot| self.slot(slot, tested))
            .collect();
        format!("[{}]", values.join(", "))
    }

    fn slot(&mut self, slot: &Slot, tested: Option<(Range, &str)>) -> Option<String> {
        Some(match slot {
            Slot::Value { kind, span } => {
                let value = match tested {
                    Some((test, variable)) if test == *span => variable.to_owned(),
                    _ => format!("({})", self.text(*span)),
                };
                match kind {
                    ValueKind::Key => format!("__tsx2rs_key{value}"),
                    ValueKind::Prop
                    | ValueKind::Style
                    | ValueKind::FlightOnly
                    | ValueKind::Child => value,
                }
            }
            Slot::Props { props, .. } => self.props(props),
            Slot::Branch {
                test,
                span,
                then,
                otherwise,
            } => {
                let variable = format!("__tsx2rs_t{}", self.depth);
                self.depth += 1;
                let condition = match test {
                    Test::Truthy => variable.clone(),
                    Test::NotNullish => format!("{variable} != null"),
                    Test::Present => "true".to_owned(),
                };
                let then = self.record(then, Some((*span, &variable)));
                let otherwise = self.record(otherwise, Some((*span, &variable)));
                self.depth -= 1;
                format!(
                    "(({variable}) => ({condition} ? [true, {then}] : [false, {otherwise}]))({})",
                    self.text(*span)
                )
            }
            Slot::List {
                array,
                callback,
                item,
            } => {
                // An item that reads nothing has an always-true condition, the encoder's to write.
                let returns = match item.as_ref() {
                    Slot::Branch {
                        test: Test::Present,
                        then,
                        ..
                    } => match then.as_slice() {
                        [Slot::Returns(returns)] => returns.clone(),
                        _ => Vec::new(),
                    },
                    Slot::Returns(returns) => returns.clone(),
                    _ => Vec::new(),
                };
                let replacements = returns_replacements(self, &returns);
                format!(
                    "({}).map({})",
                    self.text(*array),
                    self.rewrite(*callback, &replacements)
                )
            }
            Slot::Returns(returns) => {
                // Only a component's top level has several returns; written by `functions`.
                let replacements = returns_replacements(self, returns);
                replacements
                    .first()
                    .map(|(_, text)| text.clone())
                    .unwrap_or_default()
            }
            Slot::Call {
                key,
                key_at,
                props,
                children,
                slots,
                ..
            } => self.pending_call(key.as_ref(), *key_at, props, children, slots),
            // The value placed is the slot's record, built by the caller.
            Slot::Placed { span, .. } => format!("({})", self.text(*span)),
            Slot::RootKey | Slot::RootWrap => return None, // the runtime's (ADR-0008)
        })
    }

    /// A props object as JSX builds it: in order, spreads included.
    fn props(&self, props: &[Prop]) -> String {
        let entries: Vec<String> = props.iter().map(|prop| self.prop(prop)).collect();
        format!("{{{}}}", entries.join(", "))
    }

    fn prop(&self, prop: &Prop) -> String {
        match prop {
            Prop::Static { name, value } => format!("{}: {}", json_key(name), value.json()),
            Prop::Dynamic { name, value } => {
                format!("{}: ({})", json_key(name), self.text(*value))
            }
            Prop::Spread(span) => format!("...({})", self.text(*span)),
        }
    }

    /// A linked component's pending call (ADR-0013): `[config, key]`, the arguments of React's
    /// `jsx(type, config, key)`, built as the JSX transform builds them. A key written after a
    /// spread goes inside `config` (the transform's `createElement` form), in its place, and the
    /// argument is `undefined`. The runtime takes the key and props from them as `jsx` does,
    /// then calls the callee's data function once this record is complete.
    fn pending_call(
        &mut self,
        key: Option<&Key>,
        key_at: usize,
        props: &[ComponentProp],
        children: &[Node],
        slots: &[(String, Vec<Slot>)],
    ) -> String {
        let source = self.source;
        let key_value = |key: &Key| match key {
            Key::Static(key) => json_key(key),
            Key::Dynamic(span) => format!(
                "({})",
                source
                    .get(span.0 as usize..span.1 as usize)
                    .unwrap_or_default()
            ),
        };
        let in_config = props
            .iter()
            .take(key_at)
            .any(|p| matches!(p, ComponentProp::Value(Prop::Spread(_))));
        let mut entries = Vec::new();
        for (i, prop) in props.iter().enumerate() {
            if let (true, Some(key), true) = (in_config, key, i == key_at) {
                entries.push(format!("\"key\": {}", key_value(key)));
            }
            match prop {
                ComponentProp::Value(prop) => entries.push(self.prop(prop)),
                // A slot's value is its record (`Slot::Placed`).
                ComponentProp::Slot { name, .. } => {
                    if let Some((_, record)) = slots.iter().find(|(p, _)| p == name) {
                        let record = self.record(record, None);
                        entries.push(format!("{}: {record}", json_key(name)));
                    }
                }
            }
        }
        if let (true, Some(key), true) = (in_config, key, key_at == props.len()) {
            entries.push(format!("\"key\": {}", key_value(key)));
        }
        // JSX children: the `children` slot, last, as JSX passes children.
        let named = |name: &str| {
            props
                .iter()
                .any(|p| matches!(p, ComponentProp::Slot { name: n, .. } if n == name))
        };
        for (prop, record) in slots.iter().filter(|(p, _)| !named(p)) {
            let record = self.record(record, None);
            entries.push(format!("{}: {record}", json_key(prop)));
        }
        let children: Vec<String> = children.iter().filter_map(|c| self.child(c)).collect();
        match children.as_slice() {
            [] => {}
            [only] => entries.push(format!("\"children\": {only}")),
            all => entries.push(format!("\"children\": [{}]", all.join(", "))),
        }
        let key = match key {
            Some(key) if !in_config => key_value(key),
            _ => "undefined".to_owned(),
        };
        format!("[{{{}}}, {key}]", entries.join(", "))
    }

    /// A linked component's child, as JSX passes it: text, a literal or an expression.
    fn child(&self, node: &Node) -> Option<String> {
        Some(match node {
            Node::Text(text) => json_key(text),
            Node::Number(n) => crate::component::json_number(*n).unwrap_or_else(|| {
                let infinite = if *n > 0.0 { "Infinity" } else { "-Infinity" };
                (if n.is_nan() { "NaN" } else { infinite }).to_owned()
            }),
            Node::Literal(Literal::Null) => "null".to_owned(),
            Node::Literal(Literal::Bool(b)) => b.to_string(),
            Node::Hole(span) => format!("({})", self.text(*span)),
            _ => return None, // linking only takes these
        })
    }
}

fn json_key(name: &str) -> String {
    crate::component::json_string(name)
}

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
//! - the root's key path and wrap (ADR-0008) aren't in it: the runtime computes them.

use std::fmt::Write as _;

use crate::lower::{Slot, Test, ValueKind};
use crate::template::{Component, Prop, Range};

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
            Slot::RootKey | Slot::RootWrap => return None, // the runtime's (ADR-0008)
        })
    }

    /// A props object as JSX builds it: in order, spreads included.
    fn props(&self, props: &[Prop]) -> String {
        let entries: Vec<String> = props
            .iter()
            .map(|prop| match prop {
                Prop::Static { name, value } => format!("{}: {}", json_key(name), value.json()),
                Prop::Dynamic { name, value } => {
                    format!("{}: ({})", json_key(name), self.text(*value))
                }
                Prop::Spread(span) => format!("...({})", self.text(*span)),
            })
            .collect();
        format!("{{{}}}", entries.join(", "))
    }
}

fn json_key(name: &str) -> String {
    crate::component::json_string(name)
}

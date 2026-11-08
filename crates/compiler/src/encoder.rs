//! The generated encoder (ADR-0016): JavaScript that writes a route's records as the engine's
//! input, one straight-line function per template, from the same `Slot` trees lowering made the
//! program from (so their orders can't diverge). It runs on `@tsx2rs/runtime/encode`.
//!
//! Each slot of a record becomes a line or a small block reading that slot's value and writing
//! it with the encoder for its kind; conditions, loops and the choice between returns become
//! `if`s and `for`s. Pending calls run the callee's data function (from the `data` array, in the
//! order of [`Encoder::callees`]) and write its record with the callee's body.

use std::fmt::Write as _;

use crate::graph::ModuleId;
use crate::lower::{Body, Lowered, Slot, Test, ValueKind};
use crate::template::Prop;
use crate::usage::Export;

/// A route's generated encoder.
#[derive(Debug)]
pub struct Encoder {
    /// The JavaScript module: `export function encode(record, data, islands)`, returning the
    /// input bytes. `islands` resolves each island (ADR-0020); without it, a route with an
    /// island throws `NotNative`.
    pub source: String,
    /// The data functions `data` must hold, in order: (module, component index).
    pub callees: Vec<(ModuleId, usize)>,
    /// The client components `islands` is asked for, by index: (source, export).
    pub islands: Vec<(String, Export)>,
}

/// The encoder for a lowered page: `record` is its data function's record.
#[must_use]
pub fn generate(lowered: &Lowered) -> Encoder {
    let templates = || std::iter::once(&lowered.record).chain(&lowered.templates);
    // With an async component, the encoder awaits its data function, and so is async throughout.
    let asynchronous = templates().any(|record| any_async(record));
    let (fn_, await_) = if asynchronous {
        ("async function", "await ")
    } else {
        ("function", "")
    };
    let mut g = Generator {
        out: String::new(),
        callees: Vec::new(),
        islands: Vec::new(),
        next: 0,
        depth: 1,
        asynchronous,
    };
    let _ = write!(
        g.out,
        "// tsx2rs encoder (generated): writes the records as the engine's input (ADR-0016).\n\
         import {{ ENTRY_KEYS, NO_KEYS, Input, attrs, boundary, call, callAsync, html, island, \
         node, primitive, reference, rootKey, style, value }} from \"@tsx2rs/runtime/encode\";\n\n\
         export {fn_} encode(record, data, islands, boundaries) {{\n  \
         const w = new Input();\n  \
         {await_}t0(w, record, ENTRY_KEYS, data, islands, boundaries);\n  return w.finish();\n}}\n",
    );
    for (i, record) in templates().enumerate() {
        let _ = write!(
            g.out,
            "\n{fn_} t{i}(w, r, k, data, islands, boundaries) {{\n"
        );
        g.record(record, "r", 0, "k", true);
        g.out.push_str("}\n");
    }
    Encoder {
        source: g.out,
        callees: g.callees,
        islands: g.islands,
    }
}

struct Generator {
    out: String,
    callees: Vec<(ModuleId, usize)>,
    islands: Vec<(String, Export)>,
    /// For unique variable names.
    next: usize,
    /// Indentation.
    depth: usize,
    /// Whether the encoder is async: calls are awaited.
    asynchronous: bool,
}

impl Generator {
    fn var(&mut self, prefix: &str) -> String {
        self.next += 1;
        format!("{prefix}{}", self.next)
    }

    fn line(&mut self, line: &str) {
        for _ in 0..self.depth {
            self.out.push_str("  ");
        }
        self.out.push_str(line);
        self.out.push('\n');
    }

    fn block(&mut self, open: &str, body: impl FnOnce(&mut Self)) {
        self.line(open);
        self.depth += 1;
        body(self);
        self.depth -= 1;
        self.line("}");
    }

    /// A record: the array `array`, whose values start at `offset`. `root`: the record of a
    /// template's root, where a component element or a conditional is the component's output.
    fn record(&mut self, slots: &[Slot], array: &str, offset: usize, keys: &str, root: bool) {
        let own_output = root && matches!(slots, [Slot::Call { .. } | Slot::Branch { .. }]);
        let mut index = offset;
        let mut wrap: Option<String> = None;
        for (i, slot) in slots.iter().enumerate() {
            if let Slot::RootWrap = slot {
                // Before the root element (the next slot, whose key is its value's first entry),
                // and after it.
                let name = if let Some(name) = &wrap {
                    name.clone()
                } else {
                    let name = self.var("wrap");
                    let next_is_root = matches!(slots.get(i + 1), Some(Slot::Keyed { .. }));
                    let own = if next_is_root {
                        format!("{array}[{index}][0] !== null")
                    } else {
                        "false".to_owned()
                    };
                    self.line(&format!("const {name} = {keys}.implicit && {own};"));
                    wrap = Some(name.clone());
                    name
                };
                self.line(&format!("w.condition({name});"));
                continue;
            }
            self.slot(slot, &format!("{array}[{index}]"), keys, own_output);
            index += 1;
        }
    }

    fn slot(&mut self, slot: &Slot, v: &str, keys: &str, root: bool) {
        match slot {
            Slot::Value { kind, .. } => {
                let encode = match kind {
                    ValueKind::Child => "node",
                    ValueKind::Prop => "primitive",
                    ValueKind::Style => "style",
                    ValueKind::FlightOnly => "value",
                };
                self.line(&format!("{encode}(w, {v});"));
            }
            Slot::Props { .. } => self.line(&format!("attrs(w, {v});")),
            Slot::Keyed {
                root: own, record, ..
            } => {
                let g = self.var("el");
                self.line(&format!("const {g} = {v};"));
                if *own {
                    self.line(&format!("primitive(w, rootKey({keys}, {g}[0]));"));
                } else {
                    self.line(&format!("primitive(w, {g}[0]);"));
                }
                self.record(record, &g, 1, "NO_KEYS", false);
            }
            Slot::Branch {
                test: Test::Present,
                then,
                ..
            } => {
                // A list item that reads nothing else: an always-true condition.
                self.line("w.condition(true);");
                if let [only] = then.as_slice() {
                    self.slot(only, v, "NO_KEYS", false);
                }
            }
            Slot::Branch {
                then, otherwise, ..
            } => {
                let b = self.var("branch");
                let inner = if root { keys } else { "NO_KEYS" };
                self.line(&format!("const {b} = {v};"));
                self.line(&format!("w.condition({b}[0]);"));
                let record = format!("{b}[1]");
                self.block(&format!("if ({b}[0]) {{"), |g| {
                    g.record(then, &record, 0, inner, root);
                });
                self.block("else {", |g| g.record(otherwise, &record, 0, inner, root));
            }
            Slot::List { item, .. } => {
                let (items, it) = (self.var("items"), self.var("item"));
                self.line(&format!("const {items} = {v};"));
                self.line(&format!("w.u32({items}.length);"));
                self.block(&format!("for (const {it} of {items}) {{"), |g| {
                    g.slot(item, &it, "NO_KEYS", false);
                });
            }
            Slot::Returns(returns) => self.returns(returns, v, keys),
            Slot::Call {
                module,
                component,
                body,
                ..
            } => self.call(
                (*module, *component),
                body,
                v,
                if root { keys } else { "NO_KEYS" },
            ),
            // The callee places the slot's record, built by the caller.
            Slot::Placed { record, .. } => self.record(record, v, 0, "NO_KEYS", false),
            Slot::RootWrap => {} // handled by `record`
            Slot::Island {
                source,
                export,
                props,
                children,
                record,
                ..
            } => self.island((source, export), props, *children, record, v),
            Slot::Suspense {
                fallback, record, ..
            } => self.suspense(fallback.as_deref(), record, v),
        }
    }

    /// A linked component's pending call: its data function from `data`, its record written with
    /// its body, inline or its template; awaited in an async encoder (ADR-0025).
    fn call(&mut self, callee: (ModuleId, usize), body: &Body, v: &str, outer: &str) {
        let index = self
            .callees
            .iter()
            .position(|c| *c == callee)
            .unwrap_or_else(|| {
                self.callees.push(callee);
                self.callees.len() - 1
            });
        let (call, arrow) = if self.asynchronous {
            ("await callAsync", "async ")
        } else {
            ("call", "")
        };
        match body {
            Body::Template(t) => self.line(&format!(
                "{call}(w, {outer}, {v}, data[{index}], (w, r, k) => t{t}(w, r, k, data, islands, boundaries));"
            )),
            Body::Inline(record) => {
                self.block(
                    &format!("{call}(w, {outer}, {v}, data[{index}], {arrow}(w, r, k) => {{"),
                    |g| g.record(record, "r", 0, "k", true),
                );
                // `block` closed the arrow function's body; close the call.
                self.out.truncate(self.out.trim_end().len());
                self.out.push_str(");\n");
            }
        }
    }

    /// An island (ADR-0020): resolved once, then its reference, key, props, HTML before, children
    /// and HTML after, in program order.
    fn island(
        &mut self,
        (source, export): (&str, &Export),
        props: &[Prop],
        children: bool,
        record: &[Slot],
        v: &str,
    ) {
        let component = (source.to_owned(), export.clone());
        let index = self
            .islands
            .iter()
            .position(|c| *c == component)
            .unwrap_or_else(|| {
                self.islands.push(component);
                self.islands.len() - 1
            });
        let (el, i) = (self.var("el"), self.var("island"));
        self.line(&format!("const {el} = {v};"));
        self.line(&format!("const {i} = island(islands, {index}, {el}[1]);"));
        self.line(&format!("reference(w, {i}.ref);"));
        self.line(&format!("primitive(w, {el}[0]);"));
        if props.iter().any(|p| matches!(p, Prop::Spread(_))) {
            // The props object, written whole.
            self.line(&format!("value(w, {el}[1]);"));
        }
        for prop in props {
            if let Prop::Static { name, .. } | Prop::Dynamic { name, .. } = prop {
                let name = crate::component::json_string(name);
                self.line(&format!("value(w, {el}[1][{name}]);"));
            }
        }
        self.line(&format!("html(w, {i}.before);"));
        if children {
            self.record(record, &el, 2, "NO_KEYS", false);
            self.line(&format!("html(w, {i}.after);"));
        }
    }

    /// A `<Suspense>` boundary (ADR-0024): resolved once, then its reference, key, markers,
    /// fallback and children, each in the output the boundary's form needs.
    fn suspense(&mut self, fallback: Option<&[Slot]>, record: &[Slot], v: &str) {
        let (el, b) = (self.var("el"), self.var("boundary"));
        self.line(&format!("const {el} = {v};"));
        self.line(&format!("const {b} = boundary(boundaries);"));
        self.line(&format!("reference(w, {b}.ref);"));
        self.line(&format!("primitive(w, {el}[0]);"));
        self.line(&format!("html(w, {b}.open);"));
        if let Some(fallback) = fallback {
            // Pending: inline (0); resolved: nowhere (2).
            self.line(&format!("w.u8({b}.pending ? 0 : 2);"));
            self.record(fallback, &format!("{el}[1]"), 0, "NO_KEYS", false);
        }
        self.line(&format!("html(w, {b}.close);"));
        // Pending: the segments output (1); resolved: inline (0).
        self.line(&format!("w.u8({b}.pending ? 1 : 0);"));
        self.line(&format!("html(w, {b}.segment);"));
        self.record(record, &el, 2, "NO_KEYS", false);
        self.line(&format!("html(w, {b}.end);"));
    }

    /// One return is just its record; several are `[index, record]`, read as `If`s in order.
    fn returns(&mut self, returns: &[(crate::template::Range, Vec<Slot>)], v: &str, keys: &str) {
        if let [(_, record)] = returns {
            return self.record(record, v, 0, keys, true);
        }
        let (i, r) = (self.var("ret"), self.var("rec"));
        self.line(&format!("const [{i}, {r}] = {v};"));
        let last = returns.len() - 1;
        for (n, (_, record)) in returns.iter().enumerate() {
            let open = match n {
                0 => format!("if ({i} === 0) {{"),
                n if n == last => "else {".to_owned(),
                n => format!("else if ({i} === {n}) {{"),
            };
            self.block(&open, |g| {
                // Conditions for the returns before this one were false.
                for _ in 0..n {
                    g.line("w.condition(false);");
                }
                if n < last {
                    g.line("w.condition(true);");
                }
                g.record(record, &r, 0, keys, true);
            });
        }
    }
}

/// Whether a record has an async component's call, anywhere in it.
fn any_async(record: &[Slot]) -> bool {
    record.iter().any(|slot| match slot {
        Slot::Call {
            is_async,
            body,
            slots,
            ..
        } => {
            *is_async
                || matches!(body, Body::Inline(record) if any_async(record))
                || slots.iter().any(|(_, record)| any_async(record))
        }
        Slot::Keyed { record, .. } | Slot::Placed { record, .. } | Slot::Island { record, .. } => {
            any_async(record)
        }
        Slot::Suspense {
            fallback, record, ..
        } => fallback.as_deref().is_some_and(any_async) || any_async(record),
        Slot::Branch {
            then, otherwise, ..
        } => any_async(then) || any_async(otherwise),
        Slot::List { item, .. } => any_async(std::slice::from_ref(item)),
        Slot::Returns(returns) => returns.iter().any(|(_, record)| any_async(record)),
        Slot::Value { .. } | Slot::Props { .. } | Slot::RootWrap => false,
    })
}

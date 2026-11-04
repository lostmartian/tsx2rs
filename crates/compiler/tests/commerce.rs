//! The Commerce product pages (tests/apps/commerce): module graph and component usages,
//! snapshotted. The research established what these pages render (tsx2rs-lab check #1): client
//! components `Gallery`, `VariantSelector`, `AddToCart`, `Link`, `Image` and `FooterMenu` inside the page
//! segment; everything else server components.

// Tests may unwrap (docs/engineering/code-standards.md); this covers helpers outside #[test] fns.
#![allow(clippy::unwrap_used)]

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use tsx2rs_compiler::component::PropValue;
use tsx2rs_compiler::graph::{ModuleKind, Target, build};
use tsx2rs_compiler::usage::{Component, Export, usages};

fn app() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/apps/commerce")
}

/// The graph (modules and what each import leads to) and every component element.
fn describe(entry: &str) -> String {
    let graph = build(&app(), Path::new(entry)).unwrap();
    let mut out = String::from("modules:\n");
    for module in &graph.modules {
        let kind = match module.kind {
            ModuleKind::Server => "server",
            ModuleKind::Client => "client",
            ModuleKind::ServerActions => "server actions",
        };
        writeln!(out, "  {} [{kind}]", module.path).unwrap();
        for import in module.imports.iter().filter(|i| !i.type_only) {
            let target = match &import.target {
                Target::Module(id) => graph.modules[id.0].path.clone(),
                Target::Package(p) => format!("package {p}"),
                Target::Asset(a) => format!("asset {a}"),
                Target::None => "-".into(),
            };
            writeln!(out, "    {} -> {target}", import.specifier).unwrap();
        }
    }
    let (usages, diagnostics) = usages(&graph);
    out.push_str("components:\n");
    for usage in &usages {
        let what = match &usage.component {
            Component::Client { source, export } => format!("CLIENT {source}#{}", name(export)),
            Component::Server { module, export } => {
                format!("server {}#{}", graph.modules[module.0].path, name(export))
            }
            Component::Local => "local".into(),
            Component::React(builtin) => format!("react {builtin}"),
            Component::Unknown => "UNKNOWN".into(),
        };
        let props: Vec<String> = usage
            .element
            .props
            .iter()
            .map(|p| match &p.value {
                PropValue::Literal(json) => format!("{}={json}", p.name),
                PropValue::Expression(_) => format!("{}={{expr}}", p.name),
                PropValue::Spread(_) => "{...spread}".into(),
            })
            .collect();
        let children = if usage.element.has_children {
            " +children"
        } else {
            ""
        };
        let tag = usage.element.member.as_ref().map_or_else(
            || usage.element.name.clone(),
            |m| format!("{}.{m}", usage.element.name),
        );
        writeln!(
            out,
            "  {}: <{tag}> -> {what} [{}]{children}",
            graph.modules[usage.module.0].path,
            props.join(" ")
        )
        .unwrap();
    }
    out.push_str("diagnostics:\n");
    for d in graph.diagnostics.iter().chain(&diagnostics) {
        writeln!(out, "  {d}").unwrap();
    }
    out
}

fn name(export: &Export) -> &str {
    match export {
        Export::Default => "default",
        Export::Named(n) => n,
    }
}

#[test]
fn product_page() {
    insta::assert_snapshot!(describe("app/product/[handle]/page.tsx"));
}

#[test]
fn product_streaming_page() {
    insta::assert_snapshot!(describe("app/product-streaming/[handle]/page.tsx"));
}

/// Every server component of the product page's modules: its returns and what its templates hold,
/// counted by kind.
fn templates(entry: &str) -> String {
    use tsx2rs_compiler::template::{ComponentProp, Node};
    #[derive(Default)]
    struct Counts {
        elements: usize,
        texts: usize,
        holes: usize,
        conditionals: usize,
        lists: usize,
        components: usize,
        element_values: Vec<String>,
    }
    fn count(node: &Node, c: &mut Counts) {
        match node {
            Node::Element(e) => {
                c.elements += 1;
                e.children.iter().for_each(|n| count(n, c));
            }
            Node::Text(_) | Node::Number(_) | Node::Literal(_) => c.texts += 1,
            Node::Fragment(children) => children.iter().for_each(|n| count(n, c)),
            Node::Hole(_) => c.holes += 1,
            Node::Conditional(cond) => {
                c.conditionals += 1;
                count(&cond.consequent, c);
                if let Some(alternate) = &cond.alternate {
                    count(alternate, c);
                }
            }
            Node::List(list) => {
                c.lists += 1;
                list.item.iter().for_each(|t| count(&t.root, c));
            }
            Node::Component(component) => {
                c.components += 1;
                for prop in &component.props {
                    if let ComponentProp::Slot { template, .. } = prop {
                        count(template, c);
                    }
                }
                component.children.iter().for_each(|n| count(n, c));
            }
            Node::ElementValue(v) => c.element_values.push(format!("{:?}", v.reason)),
        }
    }
    let graph = build(&app(), Path::new(entry)).unwrap();
    let mut out = String::new();
    for diagnostic in &graph.diagnostics {
        writeln!(out, "! {diagnostic}").unwrap();
    }
    for module in graph.modules.iter().filter(|m| !m.components.is_empty()) {
        writeln!(out, "{}", module.path).unwrap();
        for component in &module.components {
            let mut c = Counts::default();
            component
                .returns
                .iter()
                .for_each(|t| count(&t.root, &mut c));
            writeln!(
                out,
                "  {}{}{}: {} returns; {} elements, {} static, {} holes, {} conditionals, {} lists, {} components, element values {:?}",
                component.name.as_deref().unwrap_or("(default)"),
                if component.is_async { " async" } else { "" },
                if component.uses_hooks { " hooks" } else { "" },
                component.returns.len(),
                c.elements,
                c.texts,
                c.holes,
                c.conditionals,
                c.lists,
                c.components,
                c.element_values,
            )
            .unwrap();
        }
    }
    out
}

#[test]
fn product_page_templates() {
    insta::assert_snapshot!(templates("app/product/[handle]/page.tsx"));
}

/// Every server component of the product page lowered: its program's size and its record's.
#[test]
fn product_page_lowers() {
    use tsx2rs_compiler::lower;
    let graph = build(&app(), Path::new("app/product/[handle]/page.tsx")).unwrap();
    let mut out = String::new();
    for module in graph.modules.iter().filter(|m| !m.components.is_empty()) {
        for component in &module.components {
            let name = component.name.as_deref().unwrap_or("(default)");
            match lower::component(component) {
                Ok(lowered) => writeln!(
                    out,
                    "{} {name}: {} ops, {} chunks, {} record slots",
                    module.path,
                    lowered.program.ops().len(),
                    lowered.program.chunks().len(),
                    lowered.record.len()
                )
                .unwrap(),
                Err(e) => writeln!(out, "{} {name}: ERROR {e}", module.path).unwrap(),
            }
        }
    }
    insta::assert_snapshot!(out);
}

#[test]
fn product_page_links() {
    use tsx2rs_compiler::graph::ModuleId;
    use tsx2rs_compiler::lower::{self, Body, Slot};
    /// The linked components in a record, by name, nested.
    fn calls(graph: &tsx2rs_compiler::graph::ModuleGraph, record: &[Slot], out: &mut Vec<String>) {
        for slot in record {
            match slot {
                Slot::Call {
                    module,
                    component,
                    body,
                    slots,
                    ..
                } => {
                    let callee = &graph.modules[module.0].components[*component];
                    let name = callee.name.clone().unwrap_or_default();
                    match body {
                        Body::Inline(record) => {
                            out.push(name);
                            calls(graph, record, out);
                        }
                        Body::Template(t) => out.push(format!("{name} (template {t})")),
                    }
                    for (prop, _) in slots {
                        out.push(format!("slot {prop}"));
                    }
                }
                Slot::Placed { record, .. } | Slot::Keyed { record, .. } => {
                    calls(graph, record, out);
                }
                Slot::Branch {
                    then, otherwise, ..
                } => {
                    calls(graph, then, out);
                    calls(graph, otherwise, out);
                }
                Slot::List { item, .. } => calls(graph, std::slice::from_ref(item), out),
                Slot::Returns(returns) => {
                    for (_, record) in returns {
                        calls(graph, record, out);
                    }
                }
                _ => {}
            }
        }
    }
    let graph = build(&app(), Path::new("app/product/[handle]/page.tsx")).unwrap();
    let mut out = String::new();
    for (m, module) in graph.modules.iter().enumerate() {
        for (i, component) in module.components.iter().enumerate() {
            let name = component.name.as_deref().unwrap_or("(default)");
            match lower::linked(&graph, ModuleId(m), i) {
                Ok(lowered) => {
                    let mut linked = Vec::new();
                    calls(&graph, &lowered.record, &mut linked);
                    writeln!(
                        out,
                        "{} {name}: {} ops; links [{}]",
                        module.path,
                        lowered.program.ops().len(),
                        linked.join(", ")
                    )
                    .unwrap();
                }
                Err(e) => writeln!(out, "{} {name}: ERROR {e}", module.path).unwrap(),
            }
        }
    }
    insta::assert_snapshot!(out);
}

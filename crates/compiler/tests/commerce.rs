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

//! Step 6's round trip (ADR-0016): Commerce's own server components, compiled from the app, with
//! props from its real test data (`commerce/cases.ts`), encoded by the generated encoder and the
//! reference, rendered by the engine, and compared with React rendering the original components.

// Tests may unwrap (docs/engineering/code-standards.md); this covers helpers outside #[test] fns.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use tsx2rs_compiler::graph::{self, ModuleGraph, ModuleId};
use tsx2rs_differential::components::{Fixtures, compile_component};

fn app() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../apps/commerce")
}

fn graph() -> ModuleGraph {
    graph::build(&app(), Path::new("app/product/[handle]/page.tsx")).unwrap()
}

/// The module `path`'s component exported as `export` (`default` or a name).
fn component(graph: &ModuleGraph, path: &str, export: &str) -> (ModuleId, usize) {
    let m = graph.modules.iter().position(|m| m.path == path).unwrap();
    let index = graph.modules[m]
        .components
        .iter()
        .position(|c| {
            if export == "default" {
                c.default_export
            } else {
                c.name.as_deref() == Some(export)
            }
        })
        .unwrap();
    (ModuleId(m), index)
}

fn cases(export: &str) -> String {
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("commerce/cases.ts");
    format!(
        r#"{{"file":"{}","export":"{export}"}}"#,
        file.display().to_string().replace('\\', "/")
    )
}

#[test]
fn commerce_components_round_trip_its_data() {
    let graph = graph();
    let mut fixtures = Fixtures::start().expect("start node");
    let mut failures = Vec::new();
    let mut total = 0;
    for (path, export, from) in [
        ("components/price.tsx", "default", "price"),
        ("components/label.tsx", "default", "label"),
        ("components/prose.tsx", "default", "prose"),
        ("components/grid/tile.tsx", "GridTileImage", "tile"),
    ] {
        let (module, index) = component(&graph, path, export);
        let compiled = compile_component(&graph, &app(), module, index).unwrap();
        let page = format!(r#"{{"module":"{path}","export":"{export}"}}"#);
        match fixtures.check_compiled(&compiled, &page, &cases(from)) {
            Ok(n) => total += n,
            Err(e) => failures.push(format!("{path}: {e}")),
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(total > 40, "{total} cases");
}

#[test]
fn stops_at_islands_without_a_resolver() {
    // `<Image {...props} />` is an island (ADR-0020): without the runtime's resolver, which learns
    // it from Next.js, the encoder can't write it.
    let graph = graph();
    let (module, index) = component(&graph, "components/grid/tile.tsx", "GridTileImage");
    let compiled = compile_component(&graph, &app(), module, index).unwrap();
    let mut fixtures = Fixtures::start().expect("start node");
    let page = r#"{"module":"components/grid/tile.tsx","export":"GridTileImage"}"#;
    let error = fixtures
        .check_compiled(&compiled, page, &cases("tileWithImage"))
        .unwrap_err();
    assert!(
        error.contains("a client component element, not learned yet"),
        "{error}"
    );
}

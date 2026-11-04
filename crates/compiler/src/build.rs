//! Compiling an app (ADR-0017): every page route, the way the adapter needs it at build time.
//!
//! For each page (`app/**/page.tsx`), the route either **compiles** (its program, its generated
//! encoder, and the data functions it calls) or **falls back** to Next.js with the reasons why. For
//! each server module, the code the adapter's loader appends to it in Next.js's own bundle: its
//! data functions, and their registration under the module's path, where the runtime finds them
//! (`globalThis[Symbol.for("tsx2rs.data")]`).

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;

use crate::graph::{self, ModuleGraph, ModuleId, ModuleKind};
use crate::{data, encoder, lower, usage};

/// An app compiled.
#[derive(Debug)]
pub struct App {
    /// Each page, in the order given.
    pub routes: Vec<Route>,
    /// By path (relative to the project, `/` separators): the code appended to each server
    /// module that has components.
    pub modules: BTreeMap<String, String>,
}

/// A page route.
#[derive(Debug)]
pub struct Route {
    /// The page's path, relative to the project (`app/product/[handle]/page.tsx`).
    pub page: String,
    /// Compiled, or why not.
    pub outcome: Outcome,
}

/// Whether a route compiled.
#[derive(Debug)]
pub enum Outcome {
    /// It compiled.
    Compiled(Box<Compiled>),
    /// It falls back to Next.js, for these reasons (each a diagnostic or an explanation).
    Fallback(Vec<String>),
}

/// A compiled route.
#[derive(Debug)]
pub struct Compiled {
    /// The program, in its binary format (`tsx2rs_ir::format`).
    pub program: Vec<u8>,
    /// The generated encoder (ADR-0016).
    pub encoder: String,
    /// The page's data function: its module's path and name. It may be `async`.
    pub entry: (String, String),
    /// The data functions the encoder's `data` array holds, in order: module path and name.
    pub callees: Vec<(String, String)>,
}

/// Where the loader's registrations go: a `Map` from module path to its data functions.
pub const REGISTRY: &str = r#"globalThis[Symbol.for("tsx2rs.data")]"#;

/// Compiles the pages `pages` (paths relative to `root`) of the app at `root`.
#[must_use]
pub fn app(root: &Path, pages: &[&str]) -> App {
    let mut modules = BTreeMap::new();
    let routes = pages
        .iter()
        .map(|page| {
            let outcome = match graph::build(root, Path::new(page)) {
                Ok(graph) => route(root, &graph, &mut modules),
                Err(e) => Outcome::Fallback(vec![e.to_string()]),
            };
            Route {
                page: (*page).to_owned(),
                outcome,
            }
        })
        .collect();
    App { routes, modules }
}

fn route(root: &Path, graph: &ModuleGraph, modules: &mut BTreeMap<String, String>) -> Outcome {
    let (_, usage) = usage::usages(graph);
    let mut reasons: Vec<String> = graph
        .diagnostics
        .iter()
        .chain(&usage)
        .map(ToString::to_string)
        .collect();
    let page = &graph.modules[0];
    if page.kind != ModuleKind::Server {
        reasons.push(format!("{} is a client page (`\"use client\"`)", page.path));
    }
    let entry = page.components.iter().position(|c| c.default_export);
    if entry.is_none() && reasons.is_empty() {
        reasons.push(format!("{} has no default-exported component", page.path));
    }
    let (Some(entry), true) = (entry, reasons.is_empty()) else {
        return Outcome::Fallback(reasons);
    };
    let lowered = match lower::linked(graph, ModuleId(0), entry) {
        Ok(lowered) => lowered,
        Err(e) => return Outcome::Fallback(vec![format!("{}: {e}", page.path)]),
    };
    let program = match lowered.program.encode() {
        Ok(program) => program,
        Err(e) => return Outcome::Fallback(vec![format!("{}: {e}", page.path)]),
    };
    if let Err(reason) = data_code(root, graph, modules) {
        return Outcome::Fallback(vec![reason]);
    }
    let encoder = encoder::generate(&lowered);
    let name = |(m, i): (ModuleId, usize)| {
        let module = &graph.modules[m.0];
        (module.path.clone(), data::name(&module.components[i]))
    };
    Outcome::Compiled(Box::new(Compiled {
        program,
        entry: name((ModuleId(0), entry)),
        callees: encoder.callees.iter().map(|c| name(*c)).collect(),
        encoder: encoder.source,
    }))
}

/// The appended code of every server module of `graph` that has components, once per module.
fn data_code(
    root: &Path,
    graph: &ModuleGraph,
    modules: &mut BTreeMap<String, String>,
) -> Result<(), String> {
    // Client and server-action modules have no components (`Module::components`).
    for (m, module) in graph.modules.iter().enumerate() {
        if module.components.is_empty() || modules.contains_key(&module.path) {
            continue;
        }
        let source = std::fs::read_to_string(root.join(&module.path))
            .map_err(|e| format!("{}: {e}", module.path))?;
        let mut records = Vec::new();
        for i in 0..module.components.len() {
            let lowered = lower::linked(graph, ModuleId(m), i)
                .map_err(|e| format!("{}: {e}", module.path))?;
            records.push(lowered.record);
        }
        let pairs: Vec<_> = module
            .components
            .iter()
            .zip(&records)
            .map(|(c, r)| (c, r.as_slice()))
            .collect();
        let mut code = data::functions(&source, &pairs);
        let names: Vec<String> = module.components.iter().map(data::name).collect();
        let _ = writeln!(
            code,
            "({REGISTRY} ??= new Map()).set({}, {{ {} }});",
            crate::component::json_string(&module.path),
            names.join(", ")
        );
        modules.insert(module.path.clone(), code);
    }
    Ok(())
}

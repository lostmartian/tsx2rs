//! What each component element in a route's server modules renders: a client component (an
//! island), a server component, a React built-in, or something the compiler can't classify.
//!
//! A tag's local name is followed through its import, and through re-exports (barrel files), to
//! the module that defines the export. Packages are classified by a table of known Next.js and
//! React exports; any other package component is reported, because guessing could render it
//! wrong.

use crate::component::ComponentUse;
use crate::diagnostic::{Code, Diagnostic};
use crate::graph::{Imported, ModuleGraph, ModuleId, ModuleKind, Target};

/// What a component element renders.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Component {
    /// A client component in the project (`"use client"` module) or a known client package export.
    Client {
        /// The client module's path, or the package specifier (`next/link`).
        source: String,
        /// The export.
        export: Export,
    },
    /// A server component exported by a server module.
    Server {
        /// The defining module.
        module: ModuleId,
        /// The export.
        export: Export,
    },
    /// Not imported: defined in the same module (or a prop or variable; resolved when lowering).
    Local,
    /// `Suspense`, `Fragment` or `StrictMode` from `react`.
    React(&'static str),
    /// A component the compiler can't classify (a diagnostic explains why).
    Unknown,
}

/// An export name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Export {
    /// `export default`
    Default,
    /// `export { Name }`
    Named(String),
}

/// A component element and what it renders.
#[derive(Clone, Debug)]
pub struct Usage<'g> {
    /// The server module whose JSX contains the element.
    pub module: ModuleId,
    /// The element.
    pub element: &'g ComponentUse,
    /// What it renders.
    pub component: Component,
}

/// Package exports known to be client components, as `(specifier, export)`.
const CLIENT_PACKAGE_EXPORTS: &[(&str, &str)] =
    &[("next/link", "default"), ("next/image", "default")];
/// React's built-in component exports.
const REACT_BUILTINS: &[&str] = &["Suspense", "Fragment", "StrictMode"];

/// Classifies every component element in the graph's server modules, in module then source
/// order, and reports the ones it can't classify.
#[must_use]
pub fn usages(graph: &ModuleGraph) -> (Vec<Usage<'_>>, Vec<Diagnostic>) {
    let mut usages = Vec::new();
    let mut diagnostics = Vec::new();
    for (index, module) in graph.modules.iter().enumerate() {
        let id = ModuleId(index);
        for element in &module.component_uses {
            let component = classify(graph, id, element, &mut diagnostics);
            usages.push(Usage {
                module: id,
                element,
                component,
            });
        }
    }
    (usages, diagnostics)
}

fn classify(
    graph: &ModuleGraph,
    id: ModuleId,
    element: &ComponentUse,
    diagnostics: &mut Vec<Diagnostic>,
) -> Component {
    let module = &graph.modules[id.0];
    let binding = module.imports.iter().find_map(|import| {
        import
            .bindings
            .iter()
            .find(|b| b.local == element.name && !b.type_only)
            .map(|b| (import, b))
    });
    let Some((import, binding)) = binding else {
        return Component::Local;
    };
    let export = match (&binding.imported, &element.member) {
        (Imported::Default, None) => Export::Default,
        (Imported::Named(name), None) => Export::Named(name.clone()),
        (Imported::Namespace, Some(member)) if !member.contains('.') => {
            Export::Named(member.clone())
        }
        _ => {
            diagnostics.push(diagnostic(
                graph,
                id,
                element,
                "a member of an imported component (`<A.B>`) other than a namespace export",
            ));
            return Component::Unknown;
        }
    };
    match &import.target {
        Target::Module(target) => resolve(graph, *target, export, element, id, diagnostics, 0),
        Target::Package(specifier) => package(specifier, &export).unwrap_or_else(|| {
            diagnostics.push(diagnostic(
                graph,
                id,
                element,
                &format!("component `{}` from package \"{specifier}\": not known to be a server or a client component", element.name),
            ));
            Component::Unknown
        }),
        Target::Asset(_) | Target::None => {
            diagnostics.push(diagnostic(graph, id, element, "component imported from a file that isn't a module"));
            Component::Unknown
        }
    }
}

fn package(specifier: &str, export: &Export) -> Option<Component> {
    let name = match export {
        Export::Default => "default",
        Export::Named(n) => n.as_str(),
    };
    if specifier == "react" {
        return REACT_BUILTINS
            .iter()
            .find(|b| **b == name)
            .map(|b| Component::React(b));
    }
    CLIENT_PACKAGE_EXPORTS
        .iter()
        .any(|(s, e)| *s == specifier && *e == name)
        .then(|| Component::Client {
            source: specifier.to_owned(),
            export: export.clone(),
        })
}

/// Follows an export of `module` to where it is defined.
fn resolve(
    graph: &ModuleGraph,
    module: ModuleId,
    export: Export,
    element: &ComponentUse,
    user: ModuleId,
    diagnostics: &mut Vec<Diagnostic>,
    depth: usize,
) -> Component {
    let target = &graph.modules[module.0];
    match target.kind {
        ModuleKind::Client => {
            return Component::Client {
                source: target.path.clone(),
                export,
            };
        }
        ModuleKind::ServerActions => {
            diagnostics.push(diagnostic(
                graph,
                user,
                element,
                "a server-actions module's export used as a component",
            ));
            return Component::Unknown;
        }
        ModuleKind::Server => {}
    }
    if depth > 32 {
        diagnostics.push(diagnostic(
            graph,
            user,
            element,
            "re-exports nested too deep (a cycle?)",
        ));
        return Component::Unknown;
    }
    let name = match &export {
        Export::Default => "default",
        Export::Named(n) => n.as_str(),
    };
    if target.exports.local.iter().any(|l| l == name) {
        return Component::Server { module, export };
    }
    let follow = |specifier: &str| {
        target
            .imports
            .iter()
            .find(|i| i.specifier == specifier && !i.type_only)
            .map(|i| &i.target)
    };
    if let Some(reexport) = target.exports.reexports.iter().find(|r| r.exported == name) {
        let next_export = match &reexport.imported {
            Imported::Default => Export::Default,
            Imported::Named(n) => Export::Named(n.clone()),
            Imported::Namespace => {
                diagnostics.push(diagnostic(
                    graph,
                    user,
                    element,
                    "a namespace re-export used as a component",
                ));
                return Component::Unknown;
            }
        };
        return match follow(&reexport.specifier) {
            Some(Target::Module(next)) => resolve(
                graph,
                *next,
                next_export,
                element,
                user,
                diagnostics,
                depth + 1,
            ),
            Some(Target::Package(specifier)) => {
                package(specifier, &next_export).unwrap_or(Component::Unknown)
            }
            _ => Component::Unknown,
        };
    }
    // `export * from`: only unambiguous with a single source (client modules' export lists are
    // unknown, since they aren't parsed).
    if export != Export::Default {
        if let [only] = target.exports.star.as_slice() {
            if let Some(Target::Module(next)) = follow(only) {
                return resolve(graph, *next, export, element, user, diagnostics, depth + 1);
            }
        } else if !target.exports.star.is_empty() {
            diagnostics.push(diagnostic(
                graph,
                user,
                element,
                &format!(
                    "`{name}` may come from any of several `export *` statements in {}",
                    target.path
                ),
            ));
            return Component::Unknown;
        }
    }
    diagnostics.push(diagnostic(
        graph,
        user,
        element,
        &format!("{} doesn't export `{name}`", target.path),
    ));
    Component::Unknown
}

fn diagnostic(
    graph: &ModuleGraph,
    module: ModuleId,
    element: &ComponentUse,
    message: &str,
) -> Diagnostic {
    Diagnostic {
        code: Code::UnknownComponent,
        message: message.to_owned(),
        file: graph.modules[module.0].path.clone().into(),
        span: element.span,
    }
}

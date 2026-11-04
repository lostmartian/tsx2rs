//! Node.js bindings for the tsx2rs compiler and engine (napi-rs).
//!
//! This crate is the only bridge to Node.js. It stays thin: convert arguments, call the
//! compiler or engine, map errors. Every export uses `catch_unwind`, so a Rust panic becomes
//! a JavaScript exception instead of aborting the process (ARCHITECTURE.md invariant 6).

use std::collections::HashMap;
use std::path::Path;

use napi::bindgen_prelude::Buffer;
use napi_derive::napi;
use tsx2rs_compiler::build::{self, Outcome};

/// The tsx2rs version of this native binary.
#[napi(catch_unwind)]
#[must_use]
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// The template-program format version this binary's engine runs.
#[napi(catch_unwind)]
#[must_use]
pub fn program_format_version() -> u16 {
    tsx2rs_ir::PROGRAM_FORMAT_VERSION
}

/// A page route, compiled or not (`tsx2rs_compiler::build::Route`).
#[napi(object)]
#[allow(missing_debug_implementations)] // holds a napi `Buffer`, which has no `Debug`
pub struct Route {
    /// The page's path, relative to the project.
    pub page: String,
    /// Whether it compiled.
    pub compiled: bool,
    /// Why it falls back (empty when it compiled).
    pub reasons: Vec<String>,
    /// The program, in its binary format.
    pub program: Option<Buffer>,
    /// The generated encoder (JavaScript).
    pub encoder: Option<String>,
    /// The page's data function.
    pub entry: Option<DataFunction>,
    /// The data functions the encoder's `data` array holds, in order.
    pub callees: Vec<DataFunction>,
}

/// A data function: the module it's in, and its name.
#[napi(object)]
#[derive(Debug)]
pub struct DataFunction {
    /// The module's path, relative to the project.
    pub module: String,
    /// The export.
    pub name: String,
}

/// An app compiled (`tsx2rs_compiler::build::App`).
#[napi(object)]
#[allow(missing_debug_implementations)] // holds `Route`s
pub struct App {
    /// Each page, in the order given.
    pub routes: Vec<Route>,
    /// By module path: the code the adapter's loader appends to that server module.
    pub modules: HashMap<String, String>,
}

/// Compiles the pages `pages` (paths relative to `root`) of the Next.js app at `root`.
#[napi(catch_unwind)]
#[must_use]
#[allow(clippy::needless_pass_by_value)] // napi-rs passes JavaScript arguments by value
pub fn compile_app(root: String, pages: Vec<String>) -> App {
    let pages: Vec<&str> = pages.iter().map(String::as_str).collect();
    let app = build::app(Path::new(&root), &pages);
    let function = |(module, name): (String, String)| DataFunction { module, name };
    let routes = app
        .routes
        .into_iter()
        .map(|route| match route.outcome {
            Outcome::Compiled(c) => {
                let c = *c;
                Route {
                    page: route.page,
                    compiled: true,
                    reasons: Vec::new(),
                    program: Some(c.program.into()),
                    encoder: Some(c.encoder),
                    entry: Some(function(c.entry)),
                    callees: c.callees.into_iter().map(function).collect(),
                }
            }
            Outcome::Fallback(reasons) => Route {
                page: route.page,
                compiled: false,
                reasons,
                program: None,
                encoder: None,
                entry: None,
                callees: Vec::new(),
            },
        })
        .collect();
    App {
        routes,
        modules: app.modules.into_iter().collect(),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn engine_supports_what_the_compiler_emits() {
        assert!(tsx2rs_engine::supports_format_version(
            tsx2rs_compiler::emitted_format_version()
        ));
    }
}

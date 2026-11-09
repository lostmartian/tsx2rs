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
use tsx2rs_engine::{Output, flight_script, render, script_string};

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

/// A template program, decoded and validated once (`tsx2rs_ir::Program`), to render per request.
#[napi(js_name = "Program")]
#[derive(Debug)]
pub struct TemplateProgram {
    program: tsx2rs_ir::Program,
}

/// What a render wrote (`tsx2rs_engine::Output`). The page's parts are bytes (UTF-8), handed to
/// JavaScript without a copy, as the response is written as bytes (ADR-0031).
#[napi(object)]
#[allow(missing_debug_implementations)] // holds napi `Buffer`s, which have no `Debug`
pub struct Rendered {
    /// The HTML.
    pub html: Buffer,
    /// The HTML of pending `<Suspense>` boundaries' content, which React sends after the shell.
    pub segments: Buffer,
    /// The Flight model of the page's own row, not yet escaped for a `<script>`.
    pub flight: Buffer,
    /// The rows React defers parts of the page to (ADR-0025), in React's task order. A reference
    /// to one is written `"$L!<n>"`, `n` counting from 1, for the runtime to replace with the id
    /// Next.js gave it.
    pub rows: Vec<Buffer>,
    /// What React DOM hoists into `<head>`, in its order (empty if nothing).
    pub head: String,
    /// The Flight resource hint rows (`:HL`, `:Hm`), which go before the Flight rows.
    pub hints: String,
}

/// Each of a page's Flight rows (the page's own first, as [`TemplateProgram::render`] returns
/// them) as it goes in a `<script>` (`tsx2rs_engine::flight_script`): escaped as Next.js escapes a
/// Flight chunk, with `ids` (the id Next.js gave each row, the page's at 0) put in place of the
/// rows' placeholders. Without `ids`, the placeholders stay.
///
/// # Errors
///
/// If a row isn't UTF-8.
#[napi(catch_unwind)]
#[allow(clippy::needless_pass_by_value)] // napi-rs passes JavaScript arguments by value
pub fn flight_scripts(rows: Vec<Buffer>, ids: Option<Vec<String>>) -> napi::Result<Vec<Buffer>> {
    let ids: Option<Vec<&str>> = ids
        .as_ref()
        .map(|ids| ids.iter().map(String::as_str).collect());
    rows.iter()
        .map(|row| {
            let row = std::str::from_utf8(row)
                .map_err(|e| napi::Error::from_reason(format!("a row isn't UTF-8: {e}")))?;
            let mut out = Vec::new();
            match &ids {
                Some(ids) => flight_script(&mut out, row, ids),
                None => script_string(&mut out, row),
            }
            Ok(out.into())
        })
        .collect()
}

#[napi]
impl TemplateProgram {
    /// Decodes a program in its binary format, as the manifest stores it.
    ///
    /// # Errors
    ///
    /// If the bytes aren't a valid program of the format version this binary runs.
    #[napi(constructor, catch_unwind)]
    #[allow(clippy::needless_pass_by_value)] // napi-rs passes JavaScript arguments by value
    pub fn new(bytes: Buffer) -> napi::Result<Self> {
        let program = tsx2rs_ir::Program::decode(&bytes)
            .map_err(|e| napi::Error::from_reason(format!("invalid program: {e}")))?;
        Ok(Self { program })
    }

    /// Renders the program with `input`, the encoder's bytes.
    ///
    /// # Errors
    ///
    /// If the input doesn't fit the program, or holds a value React rejects or the engine
    /// doesn't render (the request then falls back to Next.js).
    #[napi(catch_unwind)]
    #[allow(clippy::needless_pass_by_value)] // napi-rs passes JavaScript arguments by value
    pub fn render(&self, input: Buffer) -> napi::Result<Rendered> {
        let mut output = Output::new();
        render(&self.program, &input, &mut output)
            .map_err(|e| napi::Error::from_reason(format!("render failed: {e}")))?;
        let mut rows = tsx2rs_engine::outline(&output.flight, &output.marks)
            .map_err(|e| napi::Error::from_reason(format!("render failed: {e}")))?
            .into_iter();
        let flight = rows.next().unwrap_or_default();
        let mut head = Vec::new();
        output.head.write(&mut head);
        let text = |bytes: Vec<u8>| {
            String::from_utf8(bytes)
                .map_err(|e| napi::Error::from_reason(format!("not UTF-8: {e}")))
        };
        // Bytes, checked to be UTF-8 (cheap next to making JavaScript strings of them).
        let bytes = |bytes: Vec<u8>| {
            std::str::from_utf8(&bytes)
                .map_err(|e| napi::Error::from_reason(format!("not UTF-8: {e}")))?;
            Ok::<_, napi::Error>(Buffer::from(bytes))
        };
        Ok(Rendered {
            html: bytes(output.html)?,
            segments: bytes(output.segments)?,
            flight: bytes(flight)?,
            rows: rows.map(bytes).collect::<napi::Result<_>>()?,
            head: text(head)?,
            hints: text(output.hints.rows().to_vec())?,
        })
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

//! Node.js bindings for the tsx2rs compiler and engine (napi-rs).
//!
//! This crate is the only bridge to Node.js. It stays thin: convert arguments, call the
//! compiler or engine, map errors. Every export uses `catch_unwind`, so a Rust panic becomes
//! a JavaScript exception instead of aborting the process (ARCHITECTURE.md invariant 6).

use napi_derive::napi;

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

#[cfg(test)]
mod tests {
    #[test]
    fn engine_supports_what_the_compiler_emits() {
        assert!(tsx2rs_engine::supports_format_version(
            tsx2rs_compiler::emitted_format_version()
        ));
    }
}

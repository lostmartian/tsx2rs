//! Compiles Next.js server component templates into template programs.
//!
//! The pipeline (parse, module graph, server/client split, setup/template split, schema
//! inference, lowering, program emission, encoder generation) is described in
//! `ARCHITECTURE.md`. Each phase is a pure function of its input.
#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod diagnostic;

/// The template-program format version this compiler emits.
#[must_use]
pub const fn emitted_format_version() -> u16 {
    tsx2rs_ir::PROGRAM_FORMAT_VERSION
}

#[cfg(test)]
mod tests {
    use super::emitted_format_version;

    #[test]
    fn emits_the_format_version_defined_by_ir() {
        assert_eq!(emitted_format_version(), tsx2rs_ir::PROGRAM_FORMAT_VERSION);
    }
}

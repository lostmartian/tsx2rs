//! Runs template programs: copies static chunks and executes ops against the encoded input,
//! writing HTML by React's server rendering rules and Flight rows by React's Flight rules.
//!
//! Escaping happens here and only here (ARCHITECTURE.md invariant 5).
#![forbid(unsafe_code)]
#![deny(missing_docs)]

/// Whether this engine can run programs written in the given format version.
///
/// The runtime calls this before using a program. On `false` it falls back to Next.js for
/// that route instead of rendering (ARCHITECTURE.md invariant 3).
#[must_use]
pub const fn supports_format_version(version: u16) -> bool {
    version == tsx2rs_ir::PROGRAM_FORMAT_VERSION
}

#[cfg(test)]
mod tests {
    use super::supports_format_version;

    #[test]
    fn supports_the_current_format_version() {
        assert!(supports_format_version(tsx2rs_ir::PROGRAM_FORMAT_VERSION));
    }

    #[test]
    fn refuses_every_other_format_version() {
        let current = tsx2rs_ir::PROGRAM_FORMAT_VERSION;
        for version in [current.wrapping_add(1), current.wrapping_sub(1)] {
            assert!(
                !supports_format_version(version),
                "version {version} must be refused"
            );
        }
    }
}

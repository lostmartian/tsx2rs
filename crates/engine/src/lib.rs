//! Runs template programs: copies static chunks and executes ops against the encoded input,
//! writing HTML by React's server rendering rules and Flight by React's Flight rules, in one
//! pass.
//!
//! Escaping happens here and only here (ARCHITECTURE.md invariant 5). Every rule cites the React
//! or Next.js source it follows.
#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod flight;
mod head;
mod hints;
mod html;
mod input;
mod js;
mod render;
mod script;
mod style;

pub use head::Head;
pub use hints::Hints;
pub use render::{MAX_VALUE_DEPTH, Output, render};
pub use script::script_string;

/// Why a render failed. The input is checked as it is read; a program is valid by construction
/// ([`tsx2rs_ir::Program::new`]).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RenderError {
    /// The input ended in the middle of a value.
    #[error("input truncated at byte {offset}")]
    Truncated {
        /// Where more bytes were expected.
        offset: usize,
    },
    /// A string that isn't valid UTF-8.
    #[error("invalid UTF-8 in the string at byte {offset}")]
    InvalidUtf8 {
        /// Where the string's bytes start.
        offset: usize,
    },
    /// An unknown tag byte for an optional or generic value.
    #[error("bad tag {tag} at byte {offset}")]
    BadTag {
        /// Where the tag is.
        offset: usize,
        /// The tag byte.
        tag: u8,
    },
    /// A bigint whose string isn't canonical decimal digits.
    #[error("bad bigint at byte {offset}")]
    BadBigInt {
        /// Where the bigint's string starts.
        offset: usize,
    },
    /// A value React itself would throw on: a bigint for a numeric attribute (React's `isNaN`
    /// throws a `TypeError`), a `style` that isn't an object, a `ref` on a host element, an
    /// invalid tag, or content an element can't have (`children` on a void element, both
    /// `children` and `dangerouslySetInnerHTML`, …). The request falls back, and Next.js reports
    /// the error.
    #[error("a value React rejects at byte {offset}")]
    InvalidValue {
        /// Where the value starts.
        offset: usize,
    },
    /// A prop an [`Op::Attributes`](tsx2rs_ir::Op::Attributes) object can't hold: content
    /// (`children`, `dangerouslySetInnerHTML`), which the template writes. Also a second
    /// `children` entry in an element node.
    #[error("unsupported prop at byte {offset}")]
    UnsupportedProp {
        /// Where the prop's name starts.
        offset: usize,
    },
    /// An element this engine doesn't render yet (ADR-0007): one React DOM hoists into
    /// `<head>` (`<title>`, `<meta>`, `<link>`, `<script>`, `<style>`, an `<img>` it preloads), or
    /// `<html>`, `<head>`, `<body>`. Also a compiled component where React only turns it into a
    /// string (inside a `<textarea>` or an `<option>`'s text). The request falls back.
    #[error("unsupported element at byte {offset}")]
    UnsupportedElement {
        /// Where the element's type starts.
        offset: usize,
    },
    /// A template node whose template doesn't exist.
    #[error("bad template {template} at byte {offset}")]
    BadTemplate {
        /// Where the node starts.
        offset: usize,
        /// The template index.
        template: u32,
    },
    /// A condition byte other than 0 or 1.
    #[error("bad condition {value} at byte {offset}")]
    BadCondition {
        /// Where the condition is.
        offset: usize,
        /// The byte.
        value: u8,
    },
    /// Generic values nested deeper than [`MAX_VALUE_DEPTH`].
    #[error("value nested too deep at byte {offset}")]
    TooDeep {
        /// Where the too-deep value starts.
        offset: usize,
    },
    /// Input left after the program finished: the encoder and the program disagree.
    #[error("input left over at byte {offset}")]
    TrailingInput {
        /// Where the unread input starts.
        offset: usize,
    },
}

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

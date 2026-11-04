//! Build-time folding (ADR-0012): the static parts of a compiled template, written by the same
//! code that renders at runtime, so React DOM's escaping and attribute rules exist once
//! (ARCHITECTURE.md invariant 5). The compiler turns the results into `Op::Static` chunks.

use crate::render::{Output, render_after};
use crate::{RenderError, html};
use tsx2rs_ir::attribute::ElementRules;
use tsx2rs_ir::{FormatError, Op, Program};

/// A static node, rendered: what an `Op::Static` for it holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Folded {
    /// The HTML.
    pub html: Vec<u8>,
    /// The Flight value.
    pub flight: Vec<u8>,
    /// The HTML starts with a non-empty text node (a text node written before it needs React's
    /// `<!-- -->` separator).
    pub text_start: bool,
    /// The HTML ends with a non-empty text node.
    pub text_end: bool,
}

/// A run of static props, rendered: the HTML attributes and the Flight `"name":value` pairs,
/// separated by commas.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FoldedProps {
    /// The HTML attributes, each with its leading space.
    pub html: Vec<u8>,
    /// The Flight pairs.
    pub flight: Vec<u8>,
}

/// Why something can't be folded.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FoldError {
    /// The engine rejects it at runtime too.
    #[error("cannot render: {0}")]
    Render(#[from] RenderError),
    /// It hoists into `<head>` or emits a Flight resource hint, so it isn't a static chunk: the
    /// compiler keeps such elements as element values (ADR-0011).
    #[error("hoists into the head or emits a resource hint")]
    Hoists,
    /// The folding program itself is invalid (a bug in this module).
    #[error("invalid folding program: {0}")]
    Program(#[from] FormatError),
}

/// Renders a static node, given in `Op::Child`'s input grammar (`tsx2rs_ir::input`), exactly as
/// the engine renders it at runtime.
///
/// # Errors
///
/// [`FoldError::Render`] if the engine rejects the node, [`FoldError::Hoists`] if it hoists or
/// emits a hint.
pub fn node(input: &[u8]) -> Result<Folded, FoldError> {
    let program = one_op(Op::Child)?;
    // Rendered as if text came just before: a separator first means the node starts with text.
    let mut output = Output::new();
    let text_end = render_after(&program, input, &mut output, true)?;
    static_only(&output)?;
    let mut html = output.html;
    let text_start = html.starts_with(html::TEXT_SEPARATOR);
    if text_start {
        html.drain(..html::TEXT_SEPARATOR.len());
    }
    // A node that writes no HTML (`""`, `null`, `[]`) has no text at either edge; the renderer's
    // state is still the "text before" it started with.
    let empty = html.is_empty();
    Ok(Folded {
        html,
        flight: output.flight,
        text_start,
        text_end: text_end && !empty,
    })
}

/// Renders a run of static props, given in `Op::Attributes`' input grammar (a count, then name
/// and value pairs), by `rules`.
///
/// # Errors
///
/// [`FoldError::Render`] if the engine rejects a prop (content, a ref, a value React throws on).
pub fn props(rules: ElementRules, input: &[u8]) -> Result<FoldedProps, FoldError> {
    let program = one_op(Op::Attributes {
        rules,
        comma_after: false,
    })?;
    let mut output = Output::new();
    render_after(&program, input, &mut output, false)?;
    static_only(&output)?;
    Ok(FoldedProps {
        html: output.html,
        flight: output.flight,
    })
}

/// A one-op program; `Program::new` accepts any op without chunk or branch references.
fn one_op(op: Op) -> Result<Program, FoldError> {
    Ok(Program::new(Vec::new(), vec![op])?)
}

fn static_only(output: &Output) -> Result<(), FoldError> {
    if output.head.is_empty() && output.hints.is_empty() {
        Ok(())
    } else {
        Err(FoldError::Hoists)
    }
}

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

/// A host element's edges, for template markup: what goes around its props and children.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tag {
    /// `<div`: the props' attributes follow.
    pub html_open: Vec<u8>,
    /// `>` then the children then `</div>`, or `/>` for a void element: what closes it.
    pub html_close: Vec<u8>,
    /// A void element (`br`, `hr`, …): written `<tag …/>`, without children.
    pub void: bool,
    /// `["$","div",`: the key follows, then `,{`, the props, and `}]`.
    pub flight_open: Vec<u8>,
}

/// The edges of a host element React DOM writes with the generic rules.
///
/// # Errors
///
/// [`FoldError::NotGeneric`] for an invalid tag or one with rules of its own (`input`, `img`,
/// `title`, …): the compiler keeps those as element values (ADR-0012).
pub fn tag(name: &str) -> Result<Tag, FoldError> {
    let void = crate::render::generic_element(name).ok_or(FoldError::NotGeneric)?;
    let mut html_open = b"<".to_vec();
    html_open.extend_from_slice(name.as_bytes());
    let html_close = if void {
        b"/>".to_vec()
    } else {
        let mut close = b"</".to_vec();
        close.extend_from_slice(name.as_bytes());
        close.push(b'>');
        close
    };
    let mut flight_open = b"[\"$\",\"".to_vec();
    flight_open.extend_from_slice(name.as_bytes()); // a valid tag needs no escaping
    flight_open.extend_from_slice(b"\",");
    Ok(Tag {
        html_open,
        html_close,
        void,
        flight_open,
    })
}

/// An element's Flight key: `null`, or the key as React's Flight server writes it (a JSON
/// string, a leading `$` escaped).
#[must_use]
pub fn key(key: Option<&str>) -> Vec<u8> {
    let mut out = Vec::new();
    match key {
        Some(key) => crate::flight::string(&mut out, key),
        None => out.extend_from_slice(b"null"),
    }
    out
}

/// A prop's Flight key, `"name":`, as React's Flight server writes object keys (`JSON.stringify`,
/// no `$` escaping).
#[must_use]
pub fn prop_key(name: &str) -> Vec<u8> {
    let mut out = Vec::new();
    crate::flight::key(&mut out, name);
    out.push(b':');
    out
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
    /// An invalid tag, or one React DOM writes by rules of its own: not template markup.
    #[error("not an element with React DOM's generic rules")]
    NotGeneric,
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

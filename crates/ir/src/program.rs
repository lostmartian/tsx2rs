//! Template programs: what the compiler emits for a template and the engine runs.
//!
//! A program is a table of precomputed byte chunks plus a table of *templates*, each a list of
//! [`Op`]s: one per compiled component that isn't linked into its caller (ADR-0008). Template 0
//! is the entry; [`Op::Call`] runs another. Ops are *semantic*
//! (a text child, an optional string attribute, a loop over a list); the engine knows how each one
//! is written in HTML and in Flight, and writes both outputs in one pass. Fully static parts of a
//! template are a single [`Op::Static`] that copies one precomputed chunk to each output.
//!
//! Values come from the input buffer the generated encoder writes ([`crate::input`]), read
//! strictly in program order: each op that reads input reads exactly one value.
//!
//! A [`Program`] can only be built through [`Program::new`], which validates it, so the engine
//! can run any `Program` without bounds or structure checks failing at render time.

use crate::FormatError;
use crate::attribute::{ElementRules, Kind};

/// Index into a program's chunk table.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ChunkId(pub u32);

/// Index into a template's op list.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct OpIndex(pub u32);

/// Index into a program's template table. Template 0 is the entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TemplateId(pub u32);

/// One instruction of a template program.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    /// Precomputed output: `html` goes to the HTML output and `flight` to the Flight output.
    ///
    /// `text_start` / `text_end` say whether the HTML chunk begins / ends with a non-empty text
    /// node, which decides React's `<!-- -->` text separators. An empty HTML chunk leaves the
    /// separator state unchanged.
    Static {
        /// Bytes for the HTML output.
        html: ChunkId,
        /// Bytes for the Flight output.
        flight: ChunkId,
        /// The HTML chunk begins with a non-empty text node.
        text_start: bool,
        /// The HTML chunk ends with a non-empty text node.
        text_end: bool,
    },
    /// Reads a string: an HTML text node (escaped, with separators) and a Flight JSON string.
    Text,
    /// Reads a number: HTML text as JavaScript's `String(n)`, a Flight number.
    Number,
    /// Reads a prop's value (a primitive): in HTML the attribute ` name="value"`, written by
    /// React DOM's rules for its [`Kind`] (or nothing); in Flight the value. The compiler gets
    /// `name` and `kind` from [`crate::attribute::classify`].
    Prop {
        /// The attribute name as written in HTML (`class`, `href`, `data-x`).
        name: ChunkId,
        /// How the value is written.
        kind: Kind,
    },
    /// Reads a props object whose names are known only at runtime (`{...props}` on a host
    /// element, ADR-0005): every prop, classified and written by React DOM's rules for `rules`
    /// ([`crate::attribute::classify`]); in Flight the props as `"name":value` pairs, separated
    /// by commas.
    ///
    /// The object holds every prop of the element except its content (`children`,
    /// `dangerouslySetInnerHTML`), which the template writes after it. A `style` prop's value
    /// is read as [`Op::Style`] reads it.
    Attributes {
        /// Whose prop rules apply.
        rules: ElementRules,
        /// Write a comma after the last pair (when there is one), because more props follow in
        /// Flight.
        comma_after: bool,
    },
    /// Reads a `style` prop's value (an object of CSS properties, or `null` / `undefined`): in
    /// HTML the attribute ` style="…"` by React DOM's rules (`pushStyleAttribute`), or nothing;
    /// in Flight the object.
    Style,
    /// Reads a string: raw HTML (`dangerouslySetInnerHTML`, not escaped); in Flight a JSON string.
    RawHtml,
    /// Reads any value (client component props): nothing in HTML; in Flight its JSON encoding.
    Value,
    /// Reads a React node whose kind is known only at runtime (a `{x}` child, ADR-0005): a
    /// string, number or bigint is a text node; `true`, `false`, `null` and `undefined` render
    /// nothing; an array renders its nodes in order; an element is written by React DOM's rules
    /// for its type (ADR-0007). In Flight, the node's value.
    ///
    /// The node renders in the document body's context: outside any `<select>`, `<noscript>`
    /// or `<picture>`. So the compiler doesn't place a `Child` inside a `<select>`.
    Child,
    /// Reads a client component's HTML as rendered by React, with its text-edge flags: written
    /// to HTML as is; nothing in Flight (the reference and props are separate ops).
    Island,
    /// Reads a count and runs the ops up to the matching [`Op::EndLoop`] that many times.
    /// `separator` is written to Flight between iterations (e.g. `,` in an array).
    Loop {
        /// The matching `EndLoop`.
        end: OpIndex,
        /// Flight bytes written between iterations.
        separator: Option<ChunkId>,
    },
    /// Ends the innermost loop.
    EndLoop,
    /// Reads a boolean: runs the following ops if true, otherwise continues at `else_`.
    If {
        /// Where to continue when the condition is false.
        else_: OpIndex,
    },
    /// Continues at `to` (always forward; ends the true branch of an `If`).
    Jump {
        /// Where to continue.
        to: OpIndex,
    },
    /// Runs another template here, reading its values from the input in order: a compiled
    /// component the compiler didn't link into its caller (recursion, ADR-0005 section 6). Writes
    /// nothing itself: a server component's output is its template's.
    Call {
        /// The template to run.
        template: TemplateId,
    },
}

impl Op {
    /// Whether this op reads a value from the input itself ([`Op::Call`]'s template may).
    #[must_use]
    pub const fn reads_input(self) -> bool {
        !matches!(
            self,
            Self::Static { .. } | Self::EndLoop | Self::Jump { .. } | Self::Call { .. }
        )
    }
}

/// A validated template program.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Program {
    chunks: Vec<Box<[u8]>>,
    templates: Vec<Vec<Op>>,
}

impl Program {
    /// Builds a program with one template.
    ///
    /// # Errors
    ///
    /// As [`Program::with_templates`].
    pub fn new(chunks: Vec<Box<[u8]>>, ops: Vec<Op>) -> Result<Self, FormatError> {
        Self::with_templates(chunks, vec![ops])
    }

    /// Builds a program from its templates (the first is the entry), checking every rule the
    /// engine relies on:
    /// - there is at least one template;
    /// - every chunk id and [`Op::Call`] template is in range;
    /// - `If` and `Jump` targets point forward, within their template, not out of their loop and
    ///   not into a loop that starts after them;
    /// - `Loop`/`EndLoop` pairs nest properly, and each `Loop.end` is its own `EndLoop`;
    /// - every loop body reads input, itself or through a template it calls, so a loop can't run
    ///   more times than the input has bytes.
    ///
    /// # Errors
    ///
    /// Returns the first rule the program breaks; a rule inside a template as
    /// [`FormatError::InTemplate`].
    pub fn with_templates(
        chunks: Vec<Box<[u8]>>,
        templates: Vec<Vec<Op>>,
    ) -> Result<Self, FormatError> {
        if templates.is_empty() {
            return Err(FormatError::NoTemplates);
        }
        let count = u32::try_from(templates.len()).map_err(|_| FormatError::TooLarge)?;
        let reads = templates_reading_input(&templates);
        for (template, ops) in templates.iter().enumerate() {
            validate(&chunks, ops, count, &reads).map_err(|error| FormatError::InTemplate {
                template: u32::try_from(template).unwrap_or(u32::MAX),
                error: Box::new(error),
            })?;
        }
        Ok(Self { chunks, templates })
    }

    /// The chunk table.
    #[must_use]
    pub fn chunks(&self) -> &[Box<[u8]>] {
        &self.chunks
    }

    /// The entry template's ops, in order.
    #[must_use]
    pub fn ops(&self) -> &[Op] {
        self.template(TemplateId(0))
    }

    /// The template table.
    #[must_use]
    pub fn templates(&self) -> &[Vec<Op>] {
        &self.templates
    }

    /// A template's ops. Every `Op::Call` template in a validated program is in range.
    #[must_use]
    pub fn template(&self, id: TemplateId) -> &[Op] {
        self.templates.get(id.0 as usize).map_or(&[], |t| t)
    }

    /// The bytes of a chunk. Every `ChunkId` in a validated program is in range.
    #[must_use]
    pub fn chunk(&self, id: ChunkId) -> &[u8] {
        self.chunks.get(id.0 as usize).map_or(&[], |c| c)
    }
}

/// Which templates read input when they run: an op that reads, or a call to a template that
/// does (a fixed point, for calls in any order and recursion).
fn templates_reading_input(templates: &[Vec<Op>]) -> Vec<bool> {
    let mut reads: Vec<bool> = templates
        .iter()
        .map(|ops| ops.iter().any(|op| op.reads_input()))
        .collect();
    let mut changed = true;
    while changed {
        changed = false;
        for (t, ops) in templates.iter().enumerate() {
            if !reads[t] && ops.iter().any(|op| calls_reading(*op, &reads)) {
                reads[t] = true;
                changed = true;
            }
        }
    }
    reads
}

/// A call to a template that reads input.
fn calls_reading(op: Op, reads: &[bool]) -> bool {
    matches!(op, Op::Call { template } if reads.get(template.0 as usize) == Some(&true))
}

fn validate(
    chunks: &[Box<[u8]>],
    ops: &[Op],
    templates: u32,
    reads: &[bool],
) -> Result<(), FormatError> {
    let op_count = u32::try_from(ops.len()).map_err(|_| FormatError::TooLarge)?;
    let chunk_in_range = |id: ChunkId, at: usize| {
        if (id.0 as usize) < chunks.len() {
            Ok(())
        } else {
            Err(FormatError::ChunkOutOfRange {
                op: at,
                chunk: id.0,
            })
        }
    };
    // Open loops: (index of the Loop op, whether its body has read input so far).
    let mut loops: Vec<(usize, bool)> = Vec::new();
    for (at, op) in ops.iter().enumerate() {
        if (op.reads_input() || calls_reading(*op, reads))
            && let Some(open) = loops.last_mut()
        {
            open.1 = true;
        }
        match *op {
            Op::Static { html, flight, .. } => {
                chunk_in_range(html, at)?;
                chunk_in_range(flight, at)?;
            }
            Op::Prop { name, .. } => chunk_in_range(name, at)?,
            Op::Call { template } => {
                if template.0 >= templates {
                    return Err(FormatError::TemplateOutOfRange {
                        op: at,
                        template: template.0,
                    });
                }
            }
            Op::Loop { end, separator } => {
                if let Some(separator) = separator {
                    chunk_in_range(separator, at)?;
                }
                if end.0 as usize <= at || end.0 >= op_count {
                    return Err(FormatError::BadTarget {
                        op: at,
                        target: end.0,
                    });
                }
                loops.push((at, false));
            }
            Op::EndLoop => {
                let (start, reads) = loops
                    .pop()
                    .ok_or(FormatError::UnmatchedEndLoop { op: at })?;
                if !matches!(ops.get(start), Some(Op::Loop { end, .. }) if end.0 as usize == at) {
                    return Err(FormatError::UnmatchedEndLoop { op: at });
                }
                if !reads {
                    return Err(FormatError::LoopReadsNothing { op: start });
                }
            }
            Op::If { else_: target } | Op::Jump { to: target } => {
                if target.0 as usize <= at || target.0 > op_count {
                    return Err(FormatError::BadTarget {
                        op: at,
                        target: target.0,
                    });
                }
                // A branch may not leave the innermost open loop (it would skip its EndLoop).
                if let Some(&(start, _)) = loops.last()
                    && let Some(Op::Loop { end, .. }) = ops.get(start)
                    && target.0 > end.0
                {
                    return Err(FormatError::BadTarget {
                        op: at,
                        target: target.0,
                    });
                }
            }
            Op::Text
            | Op::Number
            | Op::Attributes { .. }
            | Op::Style
            | Op::RawHtml
            | Op::Value
            | Op::Child
            | Op::Island => {}
        }
    }
    if let Some(&(start, _)) = loops.last() {
        return Err(FormatError::UnclosedLoop { op: start });
    }
    check_branches_into_loops(ops)
}

/// A branch may not land inside (or on the end of) a loop that starts after it: the engine would
/// reach that loop's `EndLoop` without having entered the loop.
fn check_branches_into_loops(ops: &[Op]) -> Result<(), FormatError> {
    for (at, op) in ops.iter().enumerate() {
        let (Op::If { else_: target } | Op::Jump { to: target }) = *op else {
            continue;
        };
        let skipped = ops.get(at + 1..target.0 as usize).unwrap_or_default();
        let lands_in_loop = skipped
            .iter()
            .any(|skipped| matches!(skipped, Op::Loop { end, .. } if end.0 >= target.0));
        if lands_in_loop {
            return Err(FormatError::BadTarget {
                op: at,
                target: target.0,
            });
        }
    }
    Ok(())
}

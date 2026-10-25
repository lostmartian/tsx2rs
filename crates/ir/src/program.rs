//! Template programs: what the compiler emits for a template and the engine runs.
//!
//! A program is a table of precomputed byte chunks plus a list of [`Op`]s. Ops are *semantic*
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

/// Index into a program's chunk table.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ChunkId(pub u32);

/// Index into a program's op list.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct OpIndex(pub u32);

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
    /// Reads an optional string: an HTML attribute ` name="value"` if it is a string (nothing for
    /// `undefined` or `null`); in Flight the value (`"…"`, `null` or `"$undefined"`).
    Attribute {
        /// The attribute name as written in HTML (`class`, `href`, `data-x`).
        name: ChunkId,
    },
    /// Reads an optional boolean: the HTML attribute ` name=""` if true; in Flight the value.
    BooleanAttribute {
        /// The attribute name as written in HTML (`disabled`, `readonly`).
        name: ChunkId,
    },
    /// Reads a string: raw HTML (`dangerouslySetInnerHTML`, not escaped); in Flight a JSON string.
    RawHtml,
    /// Reads any value (client component props): nothing in HTML; in Flight its JSON encoding.
    Value,
    /// Reads a React node whose kind is known only at runtime (a `{x}` child, ADR-0005): a
    /// string, number or bigint is a text node; `true`, `false`, `null` and `undefined` render
    /// nothing; an array renders its nodes in order. In Flight, the node's value.
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
}

impl Op {
    /// Whether this op reads a value from the input.
    #[must_use]
    pub const fn reads_input(self) -> bool {
        !matches!(
            self,
            Self::Static { .. } | Self::EndLoop | Self::Jump { .. }
        )
    }
}

/// A validated template program.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Program {
    chunks: Vec<Box<[u8]>>,
    ops: Vec<Op>,
}

impl Program {
    /// Builds a program, checking every rule the engine relies on:
    /// - every chunk id is in range;
    /// - `If` and `Jump` targets point forward, within the program, not out of their loop and not
    ///   into a loop that starts after them;
    /// - `Loop`/`EndLoop` pairs nest properly, and each `Loop.end` is its own `EndLoop`;
    /// - every loop body reads input, so a loop can't run more times than the input has bytes.
    ///
    /// # Errors
    ///
    /// Returns the first rule the program breaks.
    pub fn new(chunks: Vec<Box<[u8]>>, ops: Vec<Op>) -> Result<Self, FormatError> {
        validate(&chunks, &ops)?;
        Ok(Self { chunks, ops })
    }

    /// The chunk table.
    #[must_use]
    pub fn chunks(&self) -> &[Box<[u8]>] {
        &self.chunks
    }

    /// The ops, in order.
    #[must_use]
    pub fn ops(&self) -> &[Op] {
        &self.ops
    }

    /// The bytes of a chunk. Every `ChunkId` in a validated program is in range.
    #[must_use]
    pub fn chunk(&self, id: ChunkId) -> &[u8] {
        self.chunks.get(id.0 as usize).map_or(&[], |c| c)
    }
}

fn validate(chunks: &[Box<[u8]>], ops: &[Op]) -> Result<(), FormatError> {
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
        if op.reads_input()
            && let Some(open) = loops.last_mut()
        {
            open.1 = true;
        }
        match *op {
            Op::Static { html, flight, .. } => {
                chunk_in_range(html, at)?;
                chunk_in_range(flight, at)?;
            }
            Op::Attribute { name } | Op::BooleanAttribute { name } => chunk_in_range(name, at)?,
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
            Op::Text | Op::Number | Op::RawHtml | Op::Value | Op::Child | Op::Island => {}
        }
    }
    if let Some(&(start, _)) = loops.last() {
        return Err(FormatError::UnclosedLoop { op: start });
    }
    // A branch may not land inside (or on the end of) a loop that starts after it: the engine
    // would reach that loop's EndLoop without having entered the loop.
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

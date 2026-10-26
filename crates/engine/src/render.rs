//! The interpreter: runs a template program over its input and writes HTML and Flight.

use crate::input::{Input, Optional};
use crate::{RenderError, flight, html};
use tsx2rs_ir::input::{island, tag};
use tsx2rs_ir::{ChunkId, Op, Program};

/// The two outputs of a render. Reuse one `Output` across renders (after [`Output::clear`]) to
/// avoid allocating per request.
#[derive(Debug, Default)]
pub struct Output {
    /// The HTML.
    pub html: Vec<u8>,
    /// The Flight (RSC) payload text for the rendered template, not yet split into rows or
    /// escaped for a `<script>` (see [`crate::script_string`]).
    pub flight: Vec<u8>,
}

impl Output {
    /// Empty outputs.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Empties both outputs, keeping their capacity.
    pub fn clear(&mut self) {
        self.html.clear();
        self.flight.clear();
    }
}

/// Generic values nest at most this deep (deeper input is rejected, not recursed into).
pub const MAX_VALUE_DEPTH: usize = 256;

/// Runs `program` over `input`, appending to `output`.
///
/// Every byte of `input` must be read: input left over means the encoder and the program
/// disagree, and is an error.
///
/// # Errors
///
/// If the input is truncated, has an invalid tag, condition or UTF-8 string, nests values
/// deeper than [`MAX_VALUE_DEPTH`], or has bytes left over. `output` then holds a partial
/// render and must be discarded.
pub fn render(program: &Program, input: &[u8], output: &mut Output) -> Result<(), RenderError> {
    let mut input = Input::new(input);
    let mut renderer = Renderer {
        program,
        out: output,
        last_text: false,
        loops: Vec::new(),
    };
    renderer.run(&mut input)?;
    if !input.is_empty() {
        return Err(RenderError::TrailingInput {
            offset: input.offset(),
        });
    }
    Ok(())
}

struct Renderer<'p, 'o> {
    program: &'p Program,
    out: &'o mut Output,
    /// Whether the last thing written to HTML was a non-empty text node (React's
    /// `textEmbedded`): the next text node then needs a `<!-- -->` separator.
    last_text: bool,
    loops: Vec<LoopFrame>,
}

struct LoopFrame {
    body: usize,
    remaining: u32,
    separator: Option<ChunkId>,
}

impl Renderer<'_, '_> {
    fn run(&mut self, input: &mut Input<'_>) -> Result<(), RenderError> {
        let ops = self.program.ops();
        let mut pc = 0;
        while let Some(&op) = ops.get(pc) {
            pc += 1;
            match op {
                Op::Static {
                    html,
                    flight,
                    text_start,
                    text_end,
                } => {
                    let html = self.program.chunk(html);
                    if !html.is_empty() {
                        if text_start {
                            self.separate_text();
                        }
                        self.out.html.extend_from_slice(html);
                        self.last_text = text_end;
                    }
                    self.out
                        .flight
                        .extend_from_slice(self.program.chunk(flight));
                }
                Op::Text => {
                    let s = input.string()?;
                    self.text(s);
                    flight::string(&mut self.out.flight, s);
                }
                Op::Number => {
                    let n = input.f64()?;
                    self.separate_text();
                    html::number(&mut self.out.html, n);
                    self.last_text = true;
                    flight::number(&mut self.out.flight, n);
                }
                Op::Attribute { name } => self.attribute(name, input)?,
                Op::BooleanAttribute { name } => self.boolean_attribute(name, input)?,
                Op::RawHtml => {
                    let s = input.string()?;
                    self.out.html.extend_from_slice(s.as_bytes());
                    self.last_text = false;
                    flight::string(&mut self.out.flight, s);
                }
                Op::Value => self.value(input, 0)?,
                Op::Child => self.child(input, 0)?,
                Op::Island => self.island(input)?,
                Op::Loop { end, separator } => {
                    let count = input.u32()?;
                    if count == 0 {
                        pc = end.0 as usize + 1;
                    } else {
                        self.loops.push(LoopFrame {
                            body: pc,
                            remaining: count - 1,
                            separator,
                        });
                    }
                }
                Op::EndLoop => self.end_loop(&mut pc),
                Op::If { else_ } => {
                    if !input.condition()? {
                        pc = else_.0 as usize;
                    }
                }
                Op::Jump { to } => pc = to.0 as usize,
            }
        }
        Ok(())
    }

    fn attribute(&mut self, name: ChunkId, input: &mut Input<'_>) -> Result<(), RenderError> {
        match input.optional_string()? {
            Optional::Some(value) => {
                self.attribute_start(name);
                html::escape(&mut self.out.html, value);
                self.out.html.push(b'"');
                flight::string(&mut self.out.flight, value);
            }
            Optional::Null => self.out.flight.extend_from_slice(b"null"),
            Optional::Undefined => self.out.flight.extend_from_slice(flight::UNDEFINED),
        }
        Ok(())
    }

    fn boolean_attribute(
        &mut self,
        name: ChunkId,
        input: &mut Input<'_>,
    ) -> Result<(), RenderError> {
        match input.optional_bool()? {
            Optional::Some(value) => {
                if value {
                    self.attribute_start(name);
                    self.out.html.push(b'"');
                }
                let json: &[u8] = if value { b"true" } else { b"false" };
                self.out.flight.extend_from_slice(json);
            }
            Optional::Null => self.out.flight.extend_from_slice(b"null"),
            Optional::Undefined => self.out.flight.extend_from_slice(flight::UNDEFINED),
        }
        Ok(())
    }

    /// A client component's HTML from React, joined to its neighbours by React's text rules.
    fn island(&mut self, input: &mut Input<'_>) -> Result<(), RenderError> {
        let markup = input.string()?;
        let flags = input.u8()?;
        if !markup.is_empty() {
            if flags & island::TEXT_START != 0 {
                self.separate_text();
            }
            self.out.html.extend_from_slice(markup.as_bytes());
            self.last_text = flags & island::TEXT_END != 0;
        }
        Ok(())
    }

    /// Runs the loop body again, or leaves the loop. Validation guarantees an open loop here.
    fn end_loop(&mut self, pc: &mut usize) {
        let Some(frame) = self.loops.last_mut() else {
            return;
        };
        if frame.remaining == 0 {
            self.loops.pop();
            return;
        }
        frame.remaining -= 1;
        *pc = frame.body;
        if let Some(separator) = frame.separator {
            self.out
                .flight
                .extend_from_slice(self.program.chunk(separator));
        }
    }

    /// A text node: nothing for `""` (React skips empty text and keeps the separator state).
    fn text(&mut self, s: &str) {
        if s.is_empty() {
            return;
        }
        self.separate_text();
        html::escape(&mut self.out.html, s);
        self.last_text = true;
    }

    fn separate_text(&mut self) {
        if self.last_text {
            self.out.html.extend_from_slice(html::TEXT_SEPARATOR);
        }
    }

    fn attribute_start(&mut self, name: ChunkId) {
        self.out.html.push(b' ');
        self.out.html.extend_from_slice(self.program.chunk(name));
        self.out.html.extend_from_slice(b"=\"");
    }

    /// A node known only at runtime, as React DOM's server renders a child (`renderNodeDestructive`
    /// in `react-dom-server`, 19.3.1) and React's Flight server serializes it.
    fn child(&mut self, input: &mut Input<'_>, depth: usize) -> Result<(), RenderError> {
        let offset = input.offset();
        if depth > MAX_VALUE_DEPTH {
            return Err(RenderError::TooDeep { offset });
        }
        match input.u8()? {
            // Booleans, `null` and `undefined` render nothing and leave the text state alone.
            tag::UNDEFINED => self.out.flight.extend_from_slice(flight::UNDEFINED),
            tag::NULL => self.out.flight.extend_from_slice(b"null"),
            tag::FALSE => self.out.flight.extend_from_slice(b"false"),
            tag::TRUE => self.out.flight.extend_from_slice(b"true"),
            tag::NUMBER => {
                let n = input.f64()?;
                self.separate_text();
                html::number(&mut self.out.html, n);
                self.last_text = true;
                flight::number(&mut self.out.flight, n);
            }
            tag::STRING => {
                let s = input.string()?;
                self.text(s);
                flight::string(&mut self.out.flight, s);
            }
            tag::BIGINT => {
                let digits = input.bigint()?;
                self.text(digits);
                flight::bigint(&mut self.out.flight, digits);
            }
            tag::ARRAY => {
                let count = input.u32()?;
                self.out.flight.push(b'[');
                for i in 0..count {
                    if i > 0 {
                        self.out.flight.push(b',');
                    }
                    self.child(input, depth + 1)?;
                }
                self.out.flight.push(b']');
            }
            // An object isn't a node here: element values come through their own op, and React
            // rejects any other object, so the encoder never writes one.
            tag => return Err(RenderError::BadTag { offset, tag }),
        }
        Ok(())
    }

    /// A generic value (client component props): Flight only.
    fn value(&mut self, input: &mut Input<'_>, depth: usize) -> Result<(), RenderError> {
        let offset = input.offset();
        if depth > MAX_VALUE_DEPTH {
            return Err(RenderError::TooDeep { offset });
        }
        let out = &mut self.out.flight;
        match input.u8()? {
            tag::UNDEFINED => out.extend_from_slice(flight::UNDEFINED),
            tag::NULL => out.extend_from_slice(b"null"),
            tag::FALSE => out.extend_from_slice(b"false"),
            tag::TRUE => out.extend_from_slice(b"true"),
            tag::NUMBER => flight::number(out, input.f64()?),
            tag::STRING => flight::string(out, input.string()?),
            tag::ARRAY => {
                let count = input.u32()?;
                self.out.flight.push(b'[');
                for i in 0..count {
                    if i > 0 {
                        self.out.flight.push(b',');
                    }
                    self.value(input, depth + 1)?;
                }
                self.out.flight.push(b']');
            }
            tag::OBJECT => {
                let count = input.u32()?;
                self.out.flight.push(b'{');
                for i in 0..count {
                    if i > 0 {
                        self.out.flight.push(b',');
                    }
                    flight::key(&mut self.out.flight, input.string()?);
                    self.out.flight.push(b':');
                    self.value(input, depth + 1)?;
                }
                self.out.flight.push(b'}');
            }
            tag => return Err(RenderError::BadTag { offset, tag }),
        }
        Ok(())
    }
}

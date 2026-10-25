//! The binary file format of a template program (what the compiler writes and the runtime
//! loads).
//!
//! ```text
//! magic      8 bytes  "TSX2RSP\0"
//! version    u16 LE   PROGRAM_FORMAT_VERSION
//! chunks     u32 LE count, then per chunk: u32 LE length + bytes
//! ops        u32 LE count, then per op: u8 opcode + operands (u32 LE each; see below)
//! ```
//!
//! Opcodes and operands: 0 `Static` (html, flight, then one flags byte: bit 0 `text_start`,
//! bit 1 `text_end`), 1 `Text`, 2 `Number`, 3 `Prop` (name, then a [`Kind`] byte),
//! 4 `Attributes` (an [`ElementRules`] byte, then a flags byte: bit 0 `comma_after`), 5
//! `RawHtml`, 6 `Value`, 7 `Island`, 8 `Loop` (end, separator + 1 or 0 for none), 9 `EndLoop`,
//! 10 `If` (else), 11 `Jump` (to), 12 `Child`, 13 `Style`. Decoding validates the program ([`Program::new`]), and encoding is
//! deterministic.

use crate::attribute::{ElementRules, Kind};
use crate::{ChunkId, FormatError, Op, OpIndex, PROGRAM_FORMAT_VERSION, Program};

/// The first bytes of every program file.
pub const MAGIC: &[u8; 8] = b"TSX2RSP\0";

const STATIC: u8 = 0;
const TEXT: u8 = 1;
const NUMBER: u8 = 2;
const PROP: u8 = 3;
const ATTRIBUTES: u8 = 4;
const RAW_HTML: u8 = 5;
const VALUE: u8 = 6;
const ISLAND: u8 = 7;
const LOOP: u8 = 8;
const END_LOOP: u8 = 9;
const IF: u8 = 10;
const JUMP: u8 = 11;
const CHILD: u8 = 12;
const STYLE: u8 = 13;

impl Program {
    /// Encodes the program in the binary format.
    ///
    /// # Errors
    ///
    /// [`FormatError::TooLarge`] if a count or length doesn't fit in a `u32`.
    pub fn encode(&self) -> Result<Vec<u8>, FormatError> {
        let mut out = Vec::new();
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&PROGRAM_FORMAT_VERSION.to_le_bytes());
        put_u32(&mut out, self.chunks().len())?;
        for chunk in self.chunks() {
            put_u32(&mut out, chunk.len())?;
            out.extend_from_slice(chunk);
        }
        put_u32(&mut out, self.ops().len())?;
        for op in self.ops() {
            match *op {
                Op::Static {
                    html,
                    flight,
                    text_start,
                    text_end,
                } => {
                    out.push(STATIC);
                    out.extend_from_slice(&html.0.to_le_bytes());
                    out.extend_from_slice(&flight.0.to_le_bytes());
                    out.push(u8::from(text_start) | (u8::from(text_end) << 1));
                }
                Op::Text => out.push(TEXT),
                Op::Number => out.push(NUMBER),
                Op::Prop { name, kind } => {
                    out.push(PROP);
                    out.extend_from_slice(&name.0.to_le_bytes());
                    out.push(kind.to_byte());
                }
                Op::Attributes { rules, comma_after } => {
                    out.push(ATTRIBUTES);
                    out.push(rules.to_byte());
                    out.push(u8::from(comma_after));
                }
                Op::RawHtml => out.push(RAW_HTML),
                Op::Value => out.push(VALUE),
                Op::Child => out.push(CHILD),
                Op::Style => out.push(STYLE),
                Op::Island => out.push(ISLAND),
                Op::Loop { end, separator } => {
                    out.push(LOOP);
                    out.extend_from_slice(&end.0.to_le_bytes());
                    let separator = separator.map_or(0, |c| c.0.checked_add(1).unwrap_or(0));
                    out.extend_from_slice(&separator.to_le_bytes());
                }
                Op::EndLoop => out.push(END_LOOP),
                Op::If { else_ } => {
                    out.push(IF);
                    out.extend_from_slice(&else_.0.to_le_bytes());
                }
                Op::Jump { to } => {
                    out.push(JUMP);
                    out.extend_from_slice(&to.0.to_le_bytes());
                }
            }
        }
        Ok(out)
    }

    /// Decodes and validates a program in the binary format.
    ///
    /// # Errors
    ///
    /// If the bytes aren't a program of this format version, are truncated or have trailing
    /// bytes, or describe a program that breaks a [`Program::new`] rule.
    pub fn decode(bytes: &[u8]) -> Result<Self, FormatError> {
        let mut r = Reader { bytes, at: 0 };
        if r.take(MAGIC.len())? != MAGIC {
            return Err(FormatError::NotAProgram);
        }
        let version = u16::from_le_bytes([r.u8()?, r.u8()?]);
        if version != PROGRAM_FORMAT_VERSION {
            return Err(FormatError::UnsupportedVersion { found: version });
        }
        let chunk_count = r.u32()?;
        let mut chunks = Vec::new();
        for _ in 0..chunk_count {
            let len = r.u32()? as usize;
            chunks.push(r.take(len)?.into());
        }
        let op_count = r.u32()?;
        let mut ops = Vec::new();
        for _ in 0..op_count {
            let at = r.at;
            ops.push(match r.u8()? {
                STATIC => {
                    let html = ChunkId(r.u32()?);
                    let flight = ChunkId(r.u32()?);
                    let flags = r.u8()?;
                    if flags > 0b11 {
                        return Err(FormatError::BadOpcode {
                            offset: at,
                            opcode: STATIC,
                        });
                    }
                    Op::Static {
                        html,
                        flight,
                        text_start: flags & 1 != 0,
                        text_end: flags & 2 != 0,
                    }
                }
                TEXT => Op::Text,
                NUMBER => Op::Number,
                PROP => {
                    let name = ChunkId(r.u32()?);
                    let bad = FormatError::BadOpcode {
                        offset: at,
                        opcode: PROP,
                    };
                    let kind = Kind::from_byte(r.u8()?).ok_or(bad)?;
                    Op::Prop { name, kind }
                }
                ATTRIBUTES => {
                    let bad = FormatError::BadOpcode {
                        offset: at,
                        opcode: ATTRIBUTES,
                    };
                    let rules = ElementRules::from_byte(r.u8()?).ok_or(bad.clone())?;
                    let comma_after = match r.u8()? {
                        0 => false,
                        1 => true,
                        _ => return Err(bad),
                    };
                    Op::Attributes { rules, comma_after }
                }
                RAW_HTML => Op::RawHtml,
                VALUE => Op::Value,
                CHILD => Op::Child,
                STYLE => Op::Style,
                ISLAND => Op::Island,
                LOOP => {
                    let end = OpIndex(r.u32()?);
                    let separator = r.u32()?.checked_sub(1).map(ChunkId);
                    Op::Loop { end, separator }
                }
                END_LOOP => Op::EndLoop,
                IF => Op::If {
                    else_: OpIndex(r.u32()?),
                },
                JUMP => Op::Jump {
                    to: OpIndex(r.u32()?),
                },
                opcode => return Err(FormatError::BadOpcode { offset: at, opcode }),
            });
        }
        if r.at != bytes.len() {
            return Err(FormatError::TrailingBytes { offset: r.at });
        }
        Program::new(chunks, ops)
    }
}

fn put_u32(out: &mut Vec<u8>, n: usize) -> Result<(), FormatError> {
    let n = u32::try_from(n).map_err(|_| FormatError::TooLarge)?;
    out.extend_from_slice(&n.to_le_bytes());
    Ok(())
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], FormatError> {
        let end = self
            .at
            .checked_add(n)
            .ok_or(FormatError::Truncated { offset: self.at })?;
        let slice = self
            .bytes
            .get(self.at..end)
            .ok_or(FormatError::Truncated { offset: self.at })?;
        self.at = end;
        Ok(slice)
    }

    fn u8(&mut self) -> Result<u8, FormatError> {
        Ok(self.take(1)?[0])
    }

    fn u32(&mut self) -> Result<u32, FormatError> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
}

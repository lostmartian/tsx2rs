//! Reads values from the input buffer in the format of [`tsx2rs_ir::input`], checking every
//! length, tag and UTF-8 sequence.

use crate::{MAX_VALUE_DEPTH, RenderError};
use tsx2rs_ir::input::tag;

/// A cursor over the input. Copying it gives a second cursor at the same place, for reading a
/// value again or ahead.
#[derive(Clone, Copy)]
pub(crate) struct Input<'a> {
    bytes: &'a [u8],
    at: usize,
}

/// A primitive JavaScript value, as props and children carry them.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Primitive<'a> {
    Undefined,
    Null,
    Bool(bool),
    Number(f64),
    String(&'a str),
    /// A bigint's canonical decimal digits.
    BigInt(&'a str),
}

impl<'a> Input<'a> {
    pub(crate) const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, at: 0 }
    }

    pub(crate) const fn offset(&self) -> usize {
        self.at
    }

    pub(crate) const fn is_empty(&self) -> bool {
        self.at == self.bytes.len()
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], RenderError> {
        let truncated = RenderError::Truncated { offset: self.at };
        let end = self.at.checked_add(n).ok_or(truncated.clone())?;
        let slice = self.bytes.get(self.at..end).ok_or(truncated)?;
        self.at = end;
        Ok(slice)
    }

    pub(crate) fn u8(&mut self) -> Result<u8, RenderError> {
        Ok(self.take(1)?[0])
    }

    pub(crate) fn u32(&mut self) -> Result<u32, RenderError> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    pub(crate) fn f64(&mut self) -> Result<f64, RenderError> {
        let b = self.take(8)?;
        Ok(f64::from_le_bytes([
            b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
        ]))
    }

    pub(crate) fn string(&mut self) -> Result<&'a str, RenderError> {
        let len = self.u32()? as usize;
        let offset = self.at;
        let bytes = self.take(len)?;
        std::str::from_utf8(bytes).map_err(|_| RenderError::InvalidUtf8 { offset })
    }

    /// A bigint's decimal digits (`-?[0-9]+`, no leading zeros), as `String(value)` writes them.
    pub(crate) fn bigint(&mut self) -> Result<&'a str, RenderError> {
        let offset = self.at;
        let s = self.string()?;
        let digits = s.strip_prefix('-').unwrap_or(s);
        let canonical = match digits.as_bytes() {
            [b'0'] => s.len() == 1,   // `-0` isn't a bigint
            [] | [b'0', ..] => false, // empty, or a leading zero
            bytes => bytes.iter().all(u8::is_ascii_digit),
        };
        if canonical {
            Ok(s)
        } else {
            Err(RenderError::BadBigInt { offset })
        }
    }

    pub(crate) fn condition(&mut self) -> Result<bool, RenderError> {
        let offset = self.at;
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            value => Err(RenderError::BadCondition { offset, value }),
        }
    }

    /// A tagged primitive (`tag::UNDEFINED` … `tag::STRING`, `tag::BIGINT`).
    pub(crate) fn primitive(&mut self) -> Result<Primitive<'a>, RenderError> {
        let offset = self.at;
        Ok(match self.u8()? {
            tag::UNDEFINED => Primitive::Undefined,
            tag::NULL => Primitive::Null,
            tag::FALSE => Primitive::Bool(false),
            tag::TRUE => Primitive::Bool(true),
            tag::NUMBER => Primitive::Number(self.f64()?),
            tag::STRING => Primitive::String(self.string()?),
            tag::BIGINT => Primitive::BigInt(self.bigint()?),
            tag => return Err(RenderError::BadTag { offset, tag }),
        })
    }

    /// Skips one tagged value of any kind, elements included, checking it as it goes.
    pub(crate) fn skip(&mut self, depth: usize) -> Result<(), RenderError> {
        let offset = self.at;
        if depth > MAX_VALUE_DEPTH {
            return Err(RenderError::TooDeep { offset });
        }
        match self.u8()? {
            tag::UNDEFINED | tag::NULL | tag::FALSE | tag::TRUE => {}
            tag::NUMBER => {
                self.f64()?;
            }
            tag::STRING => {
                self.string()?;
            }
            tag::BIGINT => {
                self.bigint()?;
            }
            tag::ARRAY => {
                for _ in 0..self.u32()? {
                    self.skip(depth + 1)?;
                }
            }
            tag::OBJECT => {
                for _ in 0..self.u32()? {
                    self.string()?;
                    self.skip(depth + 1)?;
                }
            }
            tag::ELEMENT => self.skip_element(depth)?,
            // Only running its template knows where a compiled component's input ends.
            tag::TEMPLATE => return Err(RenderError::UnsupportedElement { offset }),
            tag => return Err(RenderError::BadTag { offset, tag }),
        }
        Ok(())
    }

    /// Skips an element node after its tag: type, key, props and children.
    pub(crate) fn skip_element(&mut self, depth: usize) -> Result<(), RenderError> {
        self.string()?;
        let offset = self.at;
        match self.u8()? {
            tag::NULL => {}
            tag::STRING => {
                self.string()?;
            }
            tag => return Err(RenderError::BadTag { offset, tag }),
        }
        let mut children = false;
        for _ in 0..self.u32()? {
            if self.string()? == "children" {
                children = true;
            } else {
                self.skip(depth + 1)?;
            }
        }
        if children {
            self.skip(depth + 1)?;
        }
        Ok(())
    }
}

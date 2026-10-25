//! Reads values from the input buffer in the format of [`tsx2rs_ir::input`], checking every
//! length, tag and UTF-8 sequence.

use crate::RenderError;
use tsx2rs_ir::input::tag;

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
}

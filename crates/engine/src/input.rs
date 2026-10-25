//! Reads values from the input buffer in the format of [`tsx2rs_ir::input`], checking every
//! length, tag and UTF-8 sequence.

use crate::RenderError;
use tsx2rs_ir::input::tag;

pub(crate) struct Input<'a> {
    bytes: &'a [u8],
    at: usize,
}

/// An optional value as `Attribute` / `BooleanAttribute` read it.
pub(crate) enum Optional<T> {
    Undefined,
    Null,
    Some(T),
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

    pub(crate) fn condition(&mut self) -> Result<bool, RenderError> {
        let offset = self.at;
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            value => Err(RenderError::BadCondition { offset, value }),
        }
    }

    pub(crate) fn optional_string(&mut self) -> Result<Optional<&'a str>, RenderError> {
        let offset = self.at;
        match self.u8()? {
            tag::UNDEFINED => Ok(Optional::Undefined),
            tag::NULL => Ok(Optional::Null),
            tag::STRING => Ok(Optional::Some(self.string()?)),
            tag => Err(RenderError::BadTag { offset, tag }),
        }
    }

    pub(crate) fn optional_bool(&mut self) -> Result<Optional<bool>, RenderError> {
        let offset = self.at;
        match self.u8()? {
            tag::UNDEFINED => Ok(Optional::Undefined),
            tag::NULL => Ok(Optional::Null),
            tag::FALSE => Ok(Optional::Some(false)),
            tag::TRUE => Ok(Optional::Some(true)),
            tag => Err(RenderError::BadTag { offset, tag }),
        }
    }
}

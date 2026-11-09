//! gzip for served responses (ADR-0033): what Next.js's `compression` middleware does for a client
//! that accepts gzip, with the compressor's state kept between responses instead of set up and
//! torn down for each.
//!
//! The output is a gzip member (RFC 1952) holding a raw deflate stream (RFC 1951) at zlib's default
//! level, 6, as the middleware's `zlib.createGzip()` uses. Its bytes needn't match zlib's: what
//! matters is that it decompresses to the document, byte for byte.

use flate2::{Compress, Compression, Crc, FlushCompress, Status};

/// The gzip header: magic, deflate, no flags, no time, no extra flags, OS "unknown" (RFC 1952).
const HEADER: [u8; 10] = [0x1f, 0x8b, 8, 0, 0, 0, 0, 0, 0, 255];

/// zlib's default level, which the middleware uses.
const LEVEL: u32 = 6;

/// Why a document couldn't be compressed (the compressor's state is wrong: a bug).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("gzip failed: {0}")]
pub struct GzipError(String);

/// A gzip compressor that keeps its state (zlib's window and hash tables, about 256 KiB) between
/// documents. One per thread.
#[allow(missing_debug_implementations)] // `flate2::Compress` has no `Debug`
pub struct Gzip {
    deflate: Compress,
}

impl Default for Gzip {
    fn default() -> Self {
        Self::new()
    }
}

impl Gzip {
    /// A compressor at zlib's default level.
    #[must_use]
    pub fn new() -> Self {
        Self {
            deflate: Compress::new(Compression::new(LEVEL), false),
        }
    }

    /// Appends to `out` the gzip of `parts`, one after the other, as one document.
    ///
    /// # Errors
    ///
    /// [`GzipError`] if the compressor fails, which it doesn't for any input.
    pub fn gzip(&mut self, parts: &[&[u8]], out: &mut Vec<u8>) -> Result<(), GzipError> {
        self.deflate.reset();
        let length: usize = parts.iter().map(|p| p.len()).sum();
        // HTML compresses to well under a third; more is reserved as it's needed.
        out.reserve(HEADER.len() + length / 3 + 64);
        out.extend_from_slice(&HEADER);
        let mut crc = Crc::new();
        for part in parts {
            crc.update(part);
            self.deflate(part, FlushCompress::None, out)?;
        }
        self.deflate(&[], FlushCompress::Finish, out)?;
        out.extend_from_slice(&crc.sum().to_le_bytes());
        // ISIZE: the length modulo 2^32 (RFC 1952), as `Crc` counts it.
        out.extend_from_slice(&crc.amount().to_le_bytes());
        Ok(())
    }

    /// Deflates all of `input` into `out`, growing it as needed; with [`FlushCompress::Finish`],
    /// until the stream ends.
    fn deflate(
        &mut self,
        mut input: &[u8],
        flush: FlushCompress,
        out: &mut Vec<u8>,
    ) -> Result<(), GzipError> {
        loop {
            if out.capacity() - out.len() < 1024 {
                out.reserve(out.capacity().max(4096));
            }
            let before = self.deflate.total_in();
            let status = self
                .deflate
                .compress_vec(input, out, flush)
                .map_err(|e| GzipError(e.to_string()))?;
            // At most `input.len()`, so it fits a `usize`.
            #[allow(clippy::cast_possible_truncation)]
            let consumed = (self.deflate.total_in() - before) as usize;
            input = &input[consumed..];
            let done = match flush {
                FlushCompress::Finish => status == Status::StreamEnd,
                _ => input.is_empty(),
            };
            if done {
                return Ok(());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Read;

    use flate2::read::GzDecoder;
    use proptest::prelude::*;

    use super::Gzip;

    fn gunzip(bytes: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        GzDecoder::new(bytes).read_to_end(&mut out).unwrap();
        out
    }

    #[test]
    fn compresses_a_document_given_in_parts() {
        let html = "<!DOCTYPE html><html><body>".to_string() + &"<p>é ✓ text</p>".repeat(2000);
        let (a, b) = html.as_bytes().split_at(1234);
        let mut gzip = Gzip::new();
        let mut out = Vec::new();
        gzip.gzip(&[a, b, b""], &mut out).unwrap();
        assert_eq!(gunzip(&out), html.as_bytes());
        assert!(out.len() < html.len() / 10, "{} bytes", out.len());
    }

    #[test]
    fn reuses_its_state_across_documents() {
        let mut gzip = Gzip::new();
        for doc in ["", "a", &"x".repeat(100_000)] {
            let mut out = Vec::new();
            gzip.gzip(&[doc.as_bytes()], &mut out).unwrap();
            assert_eq!(gunzip(&out), doc.as_bytes());
        }
    }

    proptest! {
        #[test]
        fn round_trips_any_bytes_in_any_parts(
            parts in prop::collection::vec(prop::collection::vec(any::<u8>(), 0..3000), 0..6),
        ) {
            let mut gzip = Gzip::new();
            let mut out = Vec::new();
            let slices: Vec<&[u8]> = parts.iter().map(Vec::as_slice).collect();
            gzip.gzip(&slices, &mut out).unwrap();
            prop_assert_eq!(gunzip(&out), parts.concat());
        }
    }
}

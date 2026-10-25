//! The shared vocabulary of tsx2rs: template programs, their binary format, and the input format
//! the generated encoder writes.
//!
//! Both the compiler (which writes programs) and the engine (which runs them) depend on this
//! crate, and nothing else defines these types. See `ARCHITECTURE.md` and ADR-0003.
#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod format;
pub mod input;
mod program;

pub use format::MAGIC;
pub use program::{ChunkId, Op, OpIndex, Program};

/// Version of the template-program format written by the compiler and read by the engine.
///
/// The engine refuses programs of any other version, and the runtime then falls back to
/// Next.js (ARCHITECTURE.md invariant 3). Version 0 means "unstable, pre-release": it changes
/// without notice until the first release.
pub const PROGRAM_FORMAT_VERSION: u16 = 0;

/// Why a program was rejected.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FormatError {
    /// The bytes don't start with [`MAGIC`].
    #[error("not a tsx2rs template program")]
    NotAProgram,
    /// The program was written in another format version.
    #[error("program format version {found} (this build reads {PROGRAM_FORMAT_VERSION})")]
    UnsupportedVersion {
        /// The version in the file.
        found: u16,
    },
    /// The bytes end in the middle of the program.
    #[error("truncated program at byte {offset}")]
    Truncated {
        /// Where more bytes were expected.
        offset: usize,
    },
    /// Bytes remain after the last op.
    #[error("trailing bytes after the program at byte {offset}")]
    TrailingBytes {
        /// Where the trailing bytes start.
        offset: usize,
    },
    /// An unknown opcode, or invalid operands for a known one.
    #[error("bad opcode {opcode} at byte {offset}")]
    BadOpcode {
        /// Where the op starts.
        offset: usize,
        /// The opcode byte.
        opcode: u8,
    },
    /// A count or length doesn't fit in a `u32`.
    #[error("program too large")]
    TooLarge,
    /// An op refers to a chunk that doesn't exist.
    #[error("op {op} refers to chunk {chunk}, which doesn't exist")]
    ChunkOutOfRange {
        /// The op's index.
        op: usize,
        /// The missing chunk.
        chunk: u32,
    },
    /// A loop end, branch or jump target that isn't allowed.
    #[error("op {op} targets op {target}, which isn't allowed")]
    BadTarget {
        /// The op's index.
        op: usize,
        /// The target.
        target: u32,
    },
    /// An `EndLoop` without its `Loop`.
    #[error("op {op} ends a loop that isn't open")]
    UnmatchedEndLoop {
        /// The op's index.
        op: usize,
    },
    /// A `Loop` without its `EndLoop`.
    #[error("loop at op {op} is never closed")]
    UnclosedLoop {
        /// The loop's index.
        op: usize,
    },
    /// A loop whose body reads no input (it could run without bound).
    #[error("loop at op {op} reads no input")]
    LoopReadsNothing {
        /// The loop's index.
        op: usize,
    },
}

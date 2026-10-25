//! Compiler diagnostics: why a route (or part of it) can't be compiled. Every diagnostic has a
//! stable code, documented in `docs/diagnostics.md`; a route with an error diagnostic falls back
//! to Next.js (ADR-0002).

use std::fmt;
use std::path::PathBuf;

/// A stable diagnostic code.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Code {
    /// The file doesn't parse as TypeScript/JavaScript with JSX.
    ParseError,
    /// A relative or aliased import that doesn't resolve to a file.
    UnresolvedImport,
    /// A dynamic `import()`: the module graph can't be known at build time.
    DynamicImport,
    /// A component element whose component can't be classified as server, client or built-in.
    UnknownComponent,
}

impl Code {
    /// The code as written in messages and docs, e.g. `TSX2RS_E0001`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ParseError => "TSX2RS_E0001",
            Self::UnresolvedImport => "TSX2RS_E0002",
            Self::DynamicImport => "TSX2RS_E0003",
            Self::UnknownComponent => "TSX2RS_E0004",
        }
    }
}

impl fmt::Display for Code {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A diagnostic at a place in a source file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    /// What kind of problem.
    pub code: Code,
    /// A human-readable explanation.
    pub message: String,
    /// The file, relative to the project root.
    pub file: PathBuf,
    /// Byte offsets of the offending source (start, end).
    pub span: (u32, u32),
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} {}:{}-{}: {}",
            self.code,
            self.file.display(),
            self.span.0,
            self.span.1,
            self.message
        )
    }
}

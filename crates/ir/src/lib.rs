//! The shared vocabulary of tsx2rs: template IR, data schema and the template-program format.
//!
//! Both the compiler (which writes programs) and the engine (which runs them) depend on this
//! crate, and nothing else defines these types. See `ARCHITECTURE.md` and ADR-0003.
#![forbid(unsafe_code)]
#![deny(missing_docs)]

/// Version of the template-program format written by the compiler and read by the engine.
///
/// The engine refuses programs of any other version, and the runtime then falls back to
/// Next.js (ARCHITECTURE.md invariant 3). Version 0 means "unstable, pre-release": it changes
/// without notice until the first release.
pub const PROGRAM_FORMAT_VERSION: u16 = 0;

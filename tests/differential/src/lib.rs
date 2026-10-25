//! Differential tests for the engine with React as the oracle (docs/engineering/testing.md,
//! layer 4): generated element trees are lowered to template programs and rendered by the
//! engine, and the same trees are rendered by React DOM's server and React's Flight server. The
//! HTML must be byte-identical, and the Flight must decode to the same tree.
//!
//! Needs Node.js and this folder's `node_modules` (`pnpm install`); run with
//! `cargo nextest run -p tsx2rs_differential`.
#![forbid(unsafe_code)]

pub mod lower;
pub mod oracle;
pub mod tree;

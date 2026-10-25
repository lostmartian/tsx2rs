//! The engine against React, for generated trees.

// Tests may unwrap (docs/engineering/code-standards.md); this covers helpers outside #[test] fns.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::{Mutex, OnceLock};

use proptest::prelude::*;
use tsx2rs_differential::lower::lower;
use tsx2rs_differential::oracle::Oracle;
use tsx2rs_differential::tree::{Tree, tree};
use tsx2rs_engine::{Output, render};

fn oracle() -> &'static Mutex<Oracle> {
    static ORACLE: OnceLock<Mutex<Oracle>> = OnceLock::new();
    ORACLE.get_or_init(|| Mutex::new(Oracle::start().expect("start the React oracle (node)")))
}

/// Renders a tree with the engine and has the oracle compare it with React's output.
fn check(tree: &Tree) -> Result<(), String> {
    let lowered = lower(tree).map_err(|e| format!("lowering produced an invalid program: {e}"))?;
    let mut output = Output::new();
    render(&lowered.program, &lowered.input, &mut output).map_err(|e| format!("engine: {e}"))?;
    let html = String::from_utf8(output.html).map_err(|e| e.to_string())?;
    let flight = String::from_utf8(output.flight).map_err(|e| e.to_string())?;
    match oracle().lock().unwrap().compare(tree, &html, &flight) {
        Ok(()) => Ok(()),
        Err(Ok(mismatch)) => Err(format!(
            "React and the engine disagree\n  tree:   {}\n  engine html:   {html}\n  engine flight: {flight}\n  oracle: {}",
            tree.to_json(),
            mismatch.0
        )),
        Err(Err(io)) => Err(format!("oracle I/O: {io}")),
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: std::env::var("DIFFERENTIAL_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(512),
        ..ProptestConfig::default()
    })]

    #[test]
    fn engine_matches_react(tree in tree()) {
        if let Err(message) = check(&tree) {
            prop_assert!(false, "{}", message);
        }
    }
}

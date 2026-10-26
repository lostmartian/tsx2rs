//! The engine against React, for generated trees.

// Tests may unwrap (docs/engineering/code-standards.md); this covers helpers outside #[test] fns.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::{Mutex, OnceLock};

use proptest::prelude::*;
use tsx2rs_differential::lower::lower;
use tsx2rs_differential::oracle::Oracle;
use tsx2rs_differential::tree::{Tree, tree};
use tsx2rs_engine::{Output, RenderError, render};

fn oracle() -> &'static Mutex<Oracle> {
    static ORACLE: OnceLock<Mutex<Oracle>> = OnceLock::new();
    ORACLE.get_or_init(|| Mutex::new(Oracle::start().expect("start the React oracle (node)")))
}

/// Renders a tree with the engine and has the oracle compare it with React's output. Where the
/// engine returns an error, React must have thrown too, or (for an element the engine doesn't
/// render yet) written something outside the element: a hoisted resource.
fn check(tree: &Tree) -> Result<(), String> {
    let lowered = lower(tree).map_err(|e| format!("lowering produced an invalid program: {e}"))?;
    let mut output = Output::new();
    let mut oracle = oracle().lock().unwrap();
    if let Err(error) = render(&lowered.program, &lowered.input, &mut output) {
        let react = oracle
            .react_html(tree)
            .map_err(|e| format!("oracle I/O: {e}"))?;
        return match (&error, react) {
            // Any error falls back, and Next.js then reports React's.
            (RenderError::InvalidValue { .. } | RenderError::UnsupportedElement { .. }, Err(_)) => {
                Ok(())
            }
            // A compiled component where React only turns it into a string.
            (RenderError::UnsupportedElement { .. }, Ok(_)) if tree.has_component_as_text() => {
                Ok(())
            }
            (RenderError::UnsupportedElement { .. }, Ok(html))
                if html.contains(r#"rel=\"preload\""#) =>
            {
                Ok(())
            }
            (_, react) => Err(format!(
                "the engine returned an error React doesn't explain\n  tree:   {}\n  engine: {error:?}\n  react:  {react:?}",
                tree.to_json()
            )),
        };
    }
    let html = String::from_utf8(output.html).map_err(|e| e.to_string())?;
    let flight = String::from_utf8(output.flight).map_err(|e| e.to_string())?;
    match oracle.compare(tree, &html, &flight) {
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

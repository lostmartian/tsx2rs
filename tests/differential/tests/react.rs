//! The engine against React, for generated trees.

// Tests may unwrap (docs/engineering/code-standards.md); this covers helpers outside #[test] fns.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::{Mutex, OnceLock};

use proptest::prelude::*;
use proptest::test_runner::{TestCaseError, TestRunner};
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
        let io = |e: std::io::Error| format!("oracle I/O: {e}");
        let explained = match (&error, oracle.react_html(tree, false).map_err(io)?) {
            // Any error falls back, and Next.js then reports React's.
            (RenderError::InvalidValue { .. } | RenderError::UnsupportedElement { .. }, Err(_)) => {
                true
            }
            // React's Flight server failed on it (an error row, `N:E{…}`, as a JSON string).
            (RenderError::InvalidValue { .. }, Ok(_))
                if oracle.react_flight(tree).map_err(io)?.contains(":E{") =>
            {
                true
            }
            // A compiled component, or an element React's Flight server hints, in children React
            // DOM only turns into a string or doesn't render.
            (RenderError::UnsupportedElement { .. }, Ok(_))
                if tree.has_unrendered_special_child() =>
            {
                true
            }
            // Something React hoists: in a document, it shows up in `<head>`.
            (RenderError::UnsupportedElement { .. }, Ok(_)) => oracle
                .react_html(tree, true)
                .map_err(io)?
                .is_ok_and(|html| head_is_filled(&html)),
            _ => false,
        };
        return if explained {
            Ok(())
        } else {
            Err(format!(
                "the engine returned an error React doesn't explain\n  tree:   {}\n  engine: {error:?}\n  react:  {:?}",
                tree.to_json(),
                oracle.react_html(tree, true).map_err(io)?
            ))
        };
    }
    // The whole document, as React writes `<html><head></head><body>{tree}</body></html>`.
    let mut head = Vec::new();
    output.head.write(&mut head);
    let html = format!(
        "<html><head>{}</head><body>{}</body></html>",
        String::from_utf8(head).map_err(|e| e.to_string())?,
        String::from_utf8(output.html).map_err(|e| e.to_string())?
    );
    let flight = String::from_utf8(output.flight).map_err(|e| e.to_string())?;
    let hints = String::from_utf8(output.hints.rows().to_vec()).map_err(|e| e.to_string())?;
    match oracle.compare(tree, &html, &flight, &hints) {
        Ok(()) => Ok(()),
        Err(Ok(mismatch)) => Err(format!(
            "React and the engine disagree\n  tree:   {}\n  engine html:   {html}\n  engine flight: {flight}\n  oracle: {}",
            tree.to_json(),
            mismatch.0
        )),
        Err(Err(io)) => Err(format!("oracle I/O: {io}")),
    }
}

/// Whether React wrote anything into the document's `<head>` (it starts empty).
fn head_is_filled(html: &str) -> bool {
    html.find("<head>")
        .is_some_and(|at| !html[at + "<head>".len()..].starts_with("</head>"))
}

#[test]
fn engine_matches_react() {
    // Shrinking a failure through these nested generators recurses deeply; on a test thread's
    // default stack it overflows instead of reporting the failure.
    let run = std::thread::Builder::new()
        .stack_size(256 << 20)
        .spawn(|| {
            let cases = std::env::var("DIFFERENTIAL_CASES")
                .ok()
                .and_then(|n| n.parse().ok())
                .unwrap_or(512);
            let config = ProptestConfig {
                cases,
                source_file: Some(file!()),
                ..ProptestConfig::default()
            };
            TestRunner::new(config)
                .run(&tree(), |tree| check(&tree).map_err(TestCaseError::fail))
                .map_err(|failure| failure.to_string())
        })
        .unwrap()
        .join()
        .unwrap();
    if let Err(failure) = run {
        panic!("{failure}");
    }
}

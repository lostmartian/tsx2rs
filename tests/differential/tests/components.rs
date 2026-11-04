//! Component fixtures against React (ADR-0012, section 6): every `components/*.tsx` is compiled,
//! run (data functions in Node.js, program in the engine) for each of its cases, and compared
//! with React rendering the original component.

// Tests may unwrap (docs/engineering/code-standards.md); this covers helpers outside #[test] fns.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;

use tsx2rs_differential::components::Fixtures;

#[test]
fn compiled_components_match_react() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("components");
    let mut paths: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "tsx"))
        .collect();
    paths.sort();
    assert!(!paths.is_empty(), "no fixtures in {}", dir.display());
    let mut fixtures = Fixtures::start().expect("start node");
    let mut failures = Vec::new();
    let mut cases = 0;
    for path in &paths {
        let name = path.file_stem().unwrap().to_string_lossy();
        let source = std::fs::read_to_string(path).unwrap();
        match fixtures.check(&source) {
            Ok(n) => cases += n,
            Err(error) => failures.push(format!("{name}: {error}")),
        }
    }
    assert!(
        failures.is_empty(),
        "fixtures that differ from React:\n{}",
        failures.join("\n")
    );
    assert!(cases >= paths.len(), "every fixture has cases");
}

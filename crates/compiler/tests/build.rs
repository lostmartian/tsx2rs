//! Compiling a whole app (ADR-0017): every page route compiled or falling back with reasons, and
//! the code the adapter's loader appends to each server module.

// Tests may unwrap (docs/engineering/code-standards.md); this covers helpers outside #[test] fns.
#![allow(clippy::unwrap_used)]

mod common;

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use common::Project;
use tsx2rs_compiler::build::{self, Outcome};

fn commerce() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/apps/commerce")
}

/// Every page's outcome, one line each (reasons indented).
fn summary(app: &build::App) -> String {
    let mut out = String::new();
    for route in &app.routes {
        match &route.outcome {
            Outcome::Compiled(c) => {
                let _ = writeln!(
                    out,
                    "{}: compiled ({} program bytes, entry {}, {} callees)",
                    route.page,
                    c.program.len(),
                    c.entry.1,
                    c.callees.len()
                );
            }
            Outcome::Fallback(reasons) => {
                let _ = writeln!(out, "{}: falls back", route.page);
                for reason in reasons {
                    let _ = writeln!(out, "  {reason}");
                }
            }
        }
    }
    out
}

#[test]
fn compiles_commerce_routes_or_says_why_not() {
    let root = commerce();
    let mut pages: Vec<String> = Vec::new();
    collect_pages(&root, &root.join("app"), &mut pages);
    pages.sort();
    let pages: Vec<&str> = pages.iter().map(String::as_str).collect();
    let app = build::app(&root, &pages);
    let mut out = summary(&app);
    let _ = writeln!(out, "modules with data functions:");
    for path in app.modules.keys() {
        let _ = writeln!(out, "  {path}");
    }
    insta::assert_snapshot!(out);
}

fn collect_pages(root: &Path, dir: &Path, out: &mut Vec<String>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            collect_pages(root, &path, out);
        } else if path.file_name().is_some_and(|n| n == "page.tsx") {
            let relative = path.strip_prefix(root).unwrap();
            out.push(relative.to_string_lossy().replace('\\', "/"));
        }
    }
}

#[test]
fn falls_back_for_client_pages_and_pages_without_a_component() {
    let p = Project::new(&[
        (
            "app/client/page.tsx",
            "\"use client\";\nexport default function Page() { return <p>c</p>; }\n",
        ),
        (
            "app/none/page.tsx",
            "export const dynamic = \"force-dynamic\";\n",
        ),
        (
            "app/ok/page.tsx",
            "export default function Page() { return <p>ok</p>; }\n",
        ),
    ]);
    let app = build::app(
        &p.root,
        &[
            "app/client/page.tsx",
            "app/none/page.tsx",
            "app/ok/page.tsx",
        ],
    );
    insta::assert_snapshot!(summary(&app));
    // Only the server page with a component gets data functions.
    assert_eq!(app.modules.keys().collect::<Vec<_>>(), ["app/ok/page.tsx"]);
    assert!(app.modules["app/ok/page.tsx"].ends_with(
        "(globalThis[Symbol.for(\"tsx2rs.data\")] ??= new Map()).set(\"app/ok/page.tsx\", { __tsx2rs_data_Page });\n"
    ));
}

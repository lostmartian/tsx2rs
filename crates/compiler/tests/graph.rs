//! Module graphs of small projects written to a temporary folder.

// Tests may unwrap (docs/engineering/code-standards.md); this covers helpers outside #[test] fns.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::fmt::Write as _;
use std::path::Path;

use common::Project;

use tsx2rs_compiler::diagnostic::Code;
use tsx2rs_compiler::graph::{Imported, ModuleGraph, ModuleKind, Target, build};

/// One line per module (`path [kind]`), then one indented line per import.
fn summary(graph: &ModuleGraph) -> String {
    let mut out = String::new();
    for module in &graph.modules {
        let kind = match module.kind {
            ModuleKind::Server => "server",
            ModuleKind::Client => "client",
            ModuleKind::ServerActions => "server actions",
        };
        writeln!(out, "{} [{kind}]", module.path).unwrap();
        for import in &module.imports {
            let target = match &import.target {
                Target::Module(id) => graph.modules[id.0].path.clone(),
                Target::Package(p) => format!("package {p}"),
                Target::Asset(a) => format!("asset {a}"),
                Target::None => "-".into(),
            };
            let bindings: Vec<String> = import
                .bindings
                .iter()
                .map(|b| {
                    let name = match &b.imported {
                        Imported::Default => "default".to_owned(),
                        Imported::Named(n) => n.clone(),
                        Imported::Namespace => "*".to_owned(),
                    };
                    let ty = if b.type_only { "type " } else { "" };
                    if name == b.local {
                        format!("{ty}{name}")
                    } else {
                        format!("{ty}{name} as {}", b.local)
                    }
                })
                .collect();
            let ty = if import.type_only { " (type)" } else { "" };
            writeln!(
                out,
                "  {} -> {target}{ty} {{{}}}",
                import.specifier,
                bindings.join(", ")
            )
            .unwrap();
        }
    }
    out
}

#[test]
fn follows_relative_imports_with_extensions_and_index_files() {
    let p = Project::new(&[
        (
            "app/page.tsx",
            "import Tile from './tile';\nimport { Price as P } from '../components/price';\nimport './side';\nexport default function Page() { return <Tile><P /></Tile>; }",
        ),
        (
            "app/tile.tsx",
            "export default function Tile() { return null; }",
        ),
        (
            "components/price/index.ts",
            "export function Price() { return null; }",
        ),
        ("app/side.ts", "console.log('side effect');"),
    ]);
    assert_eq!(
        summary(&p.graph("app/page.tsx")),
        "app/page.tsx [server]\n  ./tile -> app/tile.tsx {default as Tile}\n  ../components/price -> components/price/index.ts {Price as P}\n  ./side -> app/side.ts {}\napp/tile.tsx [server]\ncomponents/price/index.ts [server]\napp/side.ts [server]\n"
    );
}

#[test]
fn resolves_tsconfig_base_url_and_paths() {
    let p = Project::new(&[
        (
            "tsconfig.json",
            r#"{ "compilerOptions": { "baseUrl": ".", "paths": { "@/*": ["src/*"] } } }"#,
        ),
        (
            "app/page.tsx",
            "import { Grid } from 'components/grid';\nimport { util } from '@/util';",
        ),
        ("components/grid.tsx", "export const Grid = 1;"),
        ("src/util.ts", "export const util = 1;"),
    ]);
    assert_eq!(
        summary(&p.graph("app/page.tsx")),
        "app/page.tsx [server]\n  components/grid -> components/grid.tsx {Grid}\n  @/util -> src/util.ts {util}\ncomponents/grid.tsx [server]\nsrc/util.ts [server]\n"
    );
}

#[test]
fn stops_at_client_boundaries_and_server_actions() {
    let p = Project::new(&[
        (
            "app/page.tsx",
            "import { Gallery } from './gallery';\nimport { addItem } from './actions';",
        ),
        (
            "app/gallery.tsx",
            "'use client';\nimport { useState } from 'react';\nimport { helper } from './browser-only';\nexport function Gallery() {}",
        ),
        ("app/browser-only.ts", "export const helper = 1;"),
        (
            "app/actions.ts",
            "\"use server\";\nimport { db } from './db';\nexport async function addItem() {}",
        ),
        ("app/db.ts", "export const db = 1;"),
    ]);
    assert_eq!(
        summary(&p.graph("app/page.tsx")),
        "app/page.tsx [server]\n  ./gallery -> app/gallery.tsx {Gallery}\n  ./actions -> app/actions.ts {addItem}\napp/gallery.tsx [client]\napp/actions.ts [server actions]\n"
    );
}

#[test]
fn a_use_client_string_after_other_statements_is_not_a_directive() {
    let p = Project::new(&[
        ("app/page.tsx", "import { X } from './x';"),
        (
            "app/x.tsx",
            "export const a = 1;\n'use client';\nexport const X = 2;",
        ),
    ]);
    assert!(summary(&p.graph("app/page.tsx")).contains("app/x.tsx [server]"));
}

#[test]
fn records_packages_and_skips_type_only_imports() {
    let p = Project::new(&[
        (
            "app/page.tsx",
            "import { Suspense } from 'react';\nimport Link from 'next/link';\nimport x from '@scope/pkg/sub';\nimport type { T } from './types';\nimport { type U } from './types2';\nimport { type V, W } from './mixed';",
        ),
        ("app/types.ts", "export type T = 1;"),
        ("app/types2.ts", "export type U = 1;"),
        ("app/mixed.ts", "export type V = 1; export const W = 1;"),
    ]);
    assert_eq!(
        summary(&p.graph("app/page.tsx")),
        "app/page.tsx [server]\n  react -> package react {Suspense}\n  next/link -> package next/link {default as Link}\n  @scope/pkg/sub -> package @scope/pkg/sub {default as x}\n  ./types -> - (type) {type T}\n  ./types2 -> - (type) {type U}\n  ./mixed -> app/mixed.ts {type V, W}\napp/mixed.ts [server]\n"
    );
}

#[test]
fn follows_re_exports() {
    let p = Project::new(&[
        (
            "app/page.tsx",
            "export { a } from './a';\nexport * from './b';\nexport * as c from './c';",
        ),
        ("app/a.ts", "export const a = 1;"),
        ("app/b.ts", "export const b = 1;"),
        ("app/c.ts", "export const c = 1;"),
    ]);
    assert_eq!(
        summary(&p.graph("app/page.tsx")),
        "app/page.tsx [server]\n  ./a -> app/a.ts {}\n  ./b -> app/b.ts {}\n  ./c -> app/c.ts {}\napp/a.ts [server]\napp/b.ts [server]\napp/c.ts [server]\n"
    );
}

#[test]
fn visits_each_module_once_in_a_cycle() {
    let p = Project::new(&[
        ("app/page.tsx", "import { a } from './a';"),
        ("app/a.ts", "import { b } from './b'; export const a = 1;"),
        ("app/b.ts", "import { a } from './a'; export const b = 1;"),
    ]);
    assert_eq!(
        summary(&p.graph("app/page.tsx")),
        "app/page.tsx [server]\n  ./a -> app/a.ts {a}\napp/a.ts [server]\n  ./b -> app/b.ts {b}\napp/b.ts [server]\n  ./a -> app/a.ts {a}\n"
    );
}

#[test]
fn records_assets_without_parsing_them() {
    let p = Project::new(&[
        (
            "app/page.tsx",
            "import './globals.css';\nimport data from './data.json';",
        ),
        ("app/globals.css", "body { color: red }"),
        ("app/data.json", "{}"),
    ]);
    assert_eq!(
        summary(&p.graph("app/page.tsx")),
        "app/page.tsx [server]\n  ./globals.css -> asset app/globals.css {}\n  ./data.json -> asset app/data.json {default as data}\n"
    );
}

#[test]
fn reports_problems_as_diagnostics() {
    let p = Project::new(&[
        (
            "app/page.tsx",
            "import { a } from './missing';\nimport { b } from './broken';\nimport { C } from './client';\nconst m = import('./lazy');",
        ),
        ("app/broken.ts", "export const = ;"),
        (
            "app/client.tsx",
            "'use client';\nconst later = import('./chunk');\nexport function C() {}",
        ),
    ]);
    let graph = p.graph("app/page.tsx");
    let found: Vec<(Code, String)> = graph
        .diagnostics
        .iter()
        .map(|d| (d.code, d.file.display().to_string()))
        .collect();
    assert_eq!(
        found,
        vec![
            (Code::DynamicImport, "app/page.tsx".to_owned()),
            (Code::UnresolvedImport, "app/page.tsx".to_owned()),
            (Code::ParseError, "app/broken.ts".to_owned()),
        ],
        "a dynamic import inside a client module is not reported (its imports aren't followed)"
    );
    let unresolved = &graph.diagnostics[1];
    let page = std::fs::read_to_string(p.root.join("app/page.tsx")).unwrap();
    let (start, end) = (unresolved.span.0 as usize, unresolved.span.1 as usize);
    assert_eq!(
        &page[start..end],
        "'./missing'",
        "the span points at the specifier"
    );
    assert_eq!(Code::ParseError.as_str(), "TSX2RS_E0001");
    assert!(
        unresolved
            .to_string()
            .starts_with("TSX2RS_E0002 app/page.tsx:")
    );
}

#[test]
fn a_missing_entry_is_an_error() {
    let p = Project::new(&[("app/other.tsx", "")]);
    assert!(build(&p.root, Path::new("app/page.tsx")).is_err());
}

#[cfg(unix)]
#[test]
fn works_when_the_project_path_goes_through_a_symlink() {
    // Regression: the resolver returns real paths, so a root given through a symlink (like
    // macOS's /var -> /private/var) made every local import look like a package.
    let p = Project::new(&[
        ("app/page.tsx", "import { a } from './a';"),
        ("app/a.ts", "export const a = 1;"),
    ]);
    let link = p.root.with_extension("link");
    let _ = std::fs::remove_file(&link);
    std::os::unix::fs::symlink(&p.root, &link).unwrap();
    let graph = build(&link, Path::new("app/page.tsx")).unwrap();
    std::fs::remove_file(&link).unwrap();
    assert_eq!(
        summary(&graph),
        "app/page.tsx [server]\n  ./a -> app/a.ts {a}\napp/a.ts [server]\n"
    );
}

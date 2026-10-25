//! Component elements: their props and children, and what they render.

// Tests may unwrap (docs/engineering/code-standards.md); this covers helpers outside #[test] fns.
#![allow(clippy::unwrap_used)]

mod common;

use common::Project;
use tsx2rs_compiler::diagnostic::Code;
use tsx2rs_compiler::usage::{Component, Export, usages};

/// `(tag, component)` for every element in the entry module, and the diagnostics' messages.
fn classify(files: &[(&str, &str)]) -> (Vec<(String, String)>, Vec<String>) {
    let p = Project::new(files);
    let graph = p.graph("app/page.tsx");
    let (found, diagnostics) = usages(&graph);
    let rows = found
        .iter()
        .map(|u| {
            let tag = u.element.member.as_ref().map_or_else(
                || u.element.name.clone(),
                |m| format!("{}.{m}", u.element.name),
            );
            let export = |e: &Export| match e {
                Export::Default => "default".to_owned(),
                Export::Named(n) => n.clone(),
            };
            let what = match &u.component {
                Component::Client { source, export: e } => format!("client {source}#{}", export(e)),
                Component::Server { module, export: e } => {
                    format!("server {}#{}", graph.modules[module.0].path, export(e))
                }
                Component::Local => "local".into(),
                Component::React(b) => format!("react {b}"),
                Component::Unknown => "unknown".into(),
            };
            (tag, what)
        })
        .collect();
    assert!(diagnostics.iter().all(|d| d.code == Code::UnknownComponent));
    (
        rows,
        diagnostics.iter().map(|d| d.message.clone()).collect(),
    )
}

fn rows(list: &[(&str, &str)]) -> Vec<(String, String)> {
    list.iter()
        .map(|(a, b)| ((*a).to_owned(), (*b).to_owned()))
        .collect()
}

#[test]
fn classifies_client_server_local_and_react_components() {
    let (found, diagnostics) = classify(&[
        (
            "app/page.tsx",
            "import { Suspense, Fragment } from 'react';\nimport Link from 'next/link';\nimport Image from 'next/image';\nimport { Gallery } from './gallery';\nimport Card from './card';\nimport * as UI from './ui';\nfunction Local() { return null; }\nexport default () => <Suspense><Fragment><Link href=\"/\" /><Image src=\"/a.png\" /><Gallery /><Card /><UI.Button /><Local /></Fragment></Suspense>;",
        ),
        (
            "app/gallery.tsx",
            "'use client';\nexport function Gallery() {}",
        ),
        (
            "app/card.tsx",
            "export default function Card() { return null; }",
        ),
        ("app/ui.tsx", "'use client';\nexport function Button() {}"),
    ]);
    assert_eq!(
        found,
        rows(&[
            ("Suspense", "react Suspense"),
            ("Fragment", "react Fragment"),
            ("Link", "client next/link#default"),
            ("Image", "client next/image#default"),
            ("Gallery", "client app/gallery.tsx#Gallery"),
            ("Card", "server app/card.tsx#default"),
            ("UI.Button", "client app/ui.tsx#Button"),
            ("Local", "local"),
        ])
    );
    assert_eq!(diagnostics, Vec::<String>::new());
}

#[test]
fn follows_re_exports_through_barrel_files() {
    let (found, diagnostics) = classify(&[
        (
            "app/page.tsx",
            "import { Gallery, Card, Tile, Spread } from './barrel';\nexport default () => <><Gallery /><Card /><Tile /><Spread /></>;",
        ),
        (
            "app/barrel.ts",
            "export { Gallery } from './gallery';\nexport { default as Card } from './card';\nexport * from './tiles';\nexport { Spread } from './nested';",
        ),
        (
            "app/gallery.tsx",
            "'use client';\nexport function Gallery() {}",
        ),
        (
            "app/card.tsx",
            "export default function Card() { return null; }",
        ),
        ("app/tiles.tsx", "export function Tile() { return null; }"),
        ("app/nested.ts", "export * from './spread';"),
        (
            "app/spread.tsx",
            "'use client';\nexport function Spread() {}",
        ),
    ]);
    assert_eq!(
        found,
        rows(&[
            ("Gallery", "client app/gallery.tsx#Gallery"),
            ("Card", "server app/card.tsx#default"),
            ("Tile", "server app/tiles.tsx#Tile"),
            ("Spread", "client app/spread.tsx#Spread"),
        ])
    );
    assert_eq!(diagnostics, Vec::<String>::new());
}

#[test]
fn reports_what_it_cannot_classify() {
    let (found, diagnostics) = classify(&[
        (
            "app/page.tsx",
            "import { Icon } from '@heroicons/react/24/outline';\nimport { Either } from './either';\nimport { action } from './actions';\nimport * as UI from './ui';\nimport { Missing } from './card';\nexport default () => <><Icon /><Either /><action /><UI.A.B /><Missing /></>;",
        ),
        (
            "app/either.ts",
            "export * from './a';\nexport * from './b';",
        ),
        ("app/a.tsx", "export function Either() {}"),
        ("app/b.tsx", "export function Other() {}"),
        (
            "app/actions.ts",
            "'use server';\nexport async function action() {}",
        ),
        ("app/ui.tsx", "'use client';\nexport const A = {};"),
        (
            "app/card.tsx",
            "export default function Card() { return null; }",
        ),
    ]);
    // <action /> is a host element to React (lowercase), so it isn't collected.
    assert_eq!(
        found
            .iter()
            .map(|(t, w)| (t.as_str(), w.as_str()))
            .collect::<Vec<_>>(),
        vec![
            ("Icon", "unknown"),
            ("Either", "unknown"),
            ("UI.A.B", "unknown"),
            ("Missing", "unknown")
        ]
    );
    assert_eq!(diagnostics.len(), 4);
    assert!(diagnostics[0].contains("from package \"@heroicons/react/24/outline\""));
    assert!(diagnostics[1].contains("several `export *`"));
    assert!(diagnostics[2].contains("member of an imported component"));
    assert!(diagnostics[3].contains("doesn't export `Missing`"));
}

#[test]
fn reports_a_server_action_used_as_a_component() {
    let (found, diagnostics) = classify(&[
        (
            "app/page.tsx",
            "import { Action } from './actions';\nexport default () => <Action />;",
        ),
        (
            "app/actions.ts",
            "'use server';\nexport async function Action() {}",
        ),
    ]);
    assert_eq!(found, rows(&[("Action", "unknown")]));
    assert!(diagnostics[0].contains("server-actions module"));
}

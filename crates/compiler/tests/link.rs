//! Linking (ADR-0012, section 5): child server components inlined into their caller's program,
//! rendered by the engine with input in the record's order, and compared with React's own output
//! for the same components (React DOM's `renderToString` and React's Flight server in production
//! mode; react 19.3.1).

// Tests may unwrap (docs/engineering/code-standards.md); this covers helpers outside #[test] fns.
#![allow(clippy::unwrap_used)]

mod common;

use common::Project;
use tsx2rs_compiler::graph::{ModuleGraph, ModuleId};
use tsx2rs_compiler::lower::{self, Lowered, Slot, ValueKind};
use tsx2rs_engine::{Output, render};
use tsx2rs_ir::input::{InputWriter, tag};

/// The graph of `files` (the first is the entry).
fn graph(files: &[(&str, &str)]) -> ModuleGraph {
    let p = Project::new(files);
    let graph = p.graph(files[0].0);
    assert!(graph.diagnostics.is_empty(), "{:?}", graph.diagnostics);
    graph
}

/// The entry's default export, lowered with linking.
fn page(graph: &ModuleGraph) -> Lowered {
    let index = graph.modules[0]
        .components
        .iter()
        .position(|c| c.default_export)
        .unwrap();
    lower::linked(graph, ModuleId(0), index).unwrap()
}

fn rendered(lowered: &Lowered, write: impl FnOnce(&mut InputWriter)) -> (String, String) {
    let mut w = InputWriter::new();
    write(&mut w);
    let mut output = Output::new();
    render(&lowered.program, &w.finish(), &mut output).unwrap();
    (
        String::from_utf8(output.html).unwrap(),
        format!("0:{}\n", String::from_utf8(output.flight).unwrap()),
    )
}

/// A root element's key-path inputs (ADR-0008): whether it's wrapped, and its key.
fn root_open(w: &mut InputWriter, wrap: bool, key: Option<&str>) {
    w.condition(wrap);
    match key {
        Some(key) => w.tag(tag::STRING).string(key),
        None => w.tag(tag::NULL),
    };
}

fn root_close(w: &mut InputWriter, wrap: bool) {
    w.condition(wrap);
}

/// Each component element in a record, in order: `call` (linked) or `child` (rendered at
/// runtime).
fn links(record: &[Slot]) -> Vec<&'static str> {
    let mut out = Vec::new();
    for slot in record {
        match slot {
            Slot::Call { record, .. } => {
                out.push("call");
                out.extend(links(record));
            }
            Slot::Value {
                kind: ValueKind::Child,
                ..
            } => out.push("child"),
            Slot::Branch {
                then, otherwise, ..
            } => {
                out.extend(links(then));
                out.extend(links(otherwise));
            }
            Slot::List { item, .. } => out.extend(links(std::slice::from_ref(item))),
            Slot::Returns(returns) => {
                for (_, record) in returns {
                    out.extend(links(record));
                }
            }
            _ => {}
        }
    }
    out
}

#[test]
fn inlines_an_imported_component() {
    let g = graph(&[
        (
            "app/page.tsx",
            r#"import { Price } from "./price";
export default function Page({ x }) {
  return <main><h1>t</h1><Price amount={x} currency="USD" />!</main>;
}"#,
        ),
        (
            "app/price.tsx",
            r#"export function Price({ amount, currency }) {
  return <p className="price">{amount}<span>{currency}</span></p>;
}"#,
        ),
    ]);
    let page = page(&g);
    let [Slot::RootWrap, Slot::RootKey, call, Slot::RootWrap] = page.record.as_slice() else {
        panic!("{:?}", page.record);
    };
    let Slot::Call {
        module,
        component,
        record,
        ..
    } = call
    else {
        panic!("{call:?}");
    };
    assert_eq!(g.modules[module.0].path, "app/price.tsx");
    assert_eq!(
        g.modules[module.0].components[*component].name.as_deref(),
        Some("Price")
    );
    assert_eq!(record.len(), 5, "{record:?}"); // wrap, key, amount, currency, wrap
    let out = rendered(&page, |w| {
        root_open(w, false, None);
        root_open(w, false, None);
        w.tag(tag::NUMBER).number(5.0);
        w.tag(tag::STRING).string("USD");
        root_close(w, false);
        root_close(w, false);
    });
    assert_eq!(
        out,
        (
            r#"<main><h1>t</h1><p class="price">5<span>USD</span></p>!</main>"#.into(),
            "0:[\"$\",\"main\",null,{\"children\":[[\"$\",\"h1\",null,{\"children\":\"t\"}],[\"$\",\"p\",null,{\"className\":\"price\",\"children\":[5,[\"$\",\"span\",null,{\"children\":\"USD\"}]]}],\"!\"]}]\n".into()
        )
    );
}

#[test]
fn separates_text_across_the_component_boundary() {
    let g = graph(&[(
        "app/page.tsx",
        r#"function Name({ name }) { return name; }
export default function Page() { return <p>a<Name name="Bob" />c</p>; }"#,
    )]);
    let page = page(&g);
    assert_eq!(links(&page.record), ["call", "child"]);
    let out = rendered(&page, |w| {
        root_open(w, false, None);
        w.tag(tag::STRING).string("Bob");
        root_close(w, false);
    });
    assert_eq!(
        out,
        (
            "<p>a<!-- -->Bob<!-- -->c</p>".into(),
            "0:[\"$\",\"p\",null,{\"children\":[\"a\",\"Bob\",\"c\"]}]\n".into()
        )
    );
}

#[test]
fn links_through_modules_and_lists_with_runtime_key_paths() {
    let g = graph(&[
        (
            "app/page.tsx",
            r#"import Item from "./item";
export default function Page({ xs }) {
  return <ul>{xs.map((i) => <Item key={i} label={"x" + i} />)}</ul>;
}"#,
        ),
        (
            "app/item.tsx",
            r#"import { Badge } from "./ui/badge";
export default function Item({ label }) { return <li><Badge label={label} /></li>; }"#,
        ),
        (
            "app/ui/badge.tsx",
            r#"export const Badge = ({ label }) => <b key="b">{label}</b>;"#,
        ),
    ]);
    let page = page(&g);
    // Item, inside it Badge (resolved from item.tsx, not page.tsx), and Badge's `{label}`.
    assert_eq!(links(&page.record), ["call", "call", "child"]);
    let out = rendered(&page, |w| {
        root_open(w, false, None);
        w.count(2);
        for i in ["1", "2"] {
            // Item's `<li>` gets the key path `i`; Badge is unkeyed and returns a keyed `<b>`,
            // so React wraps it in an array (ADR-0008).
            root_open(w, false, Some(i));
            root_open(w, true, Some("b"));
            w.tag(tag::STRING).string(&format!("x{i}"));
            root_close(w, true);
            root_close(w, false);
        }
        root_close(w, false);
    });
    assert_eq!(
        out,
        (
            "<ul><li><b>x1</b></li><li><b>x2</b></li></ul>".into(),
            "0:[\"$\",\"ul\",null,{\"children\":[[\"$\",\"li\",\"1\",{\"children\":[[\"$\",\"b\",\"b\",{\"children\":\"x1\"}]]}],[\"$\",\"li\",\"2\",{\"children\":[[\"$\",\"b\",\"b\",{\"children\":\"x2\"}]]}]]}]\n".into()
        )
    );
}

#[test]
fn leaves_what_it_cannot_link_to_the_runtime() {
    let g = graph(&[
        (
            "app/page.tsx",
            r#"import { use } from "react";
import { Counter } from "./counter";
function Tree({ n }) { return <ul>{n > 0 && <Tree n={n - 1} />}</ul>; }
function Hooked({ p }) { const v = use(p); return <i>{v}</i>; }
async function Later() { return <s>later</s>; }
function Box({ children, title }) { return <div>{title}{children}</div>; }
function Shadowed() { return <u>never linked</u>; }
export default function Page({ p, Shadowed }) {
  return (
    <main>
      <Tree n={2} />
      <Hooked p={p} />
      <Later />
      <Box title={<b>jsx prop</b>} />
      <Box><b>jsx child</b></Box>
      <Box title="t">text child {p}</Box>
      <Shadowed />
      <Counter />
    </main>
  );
}"#,
        ),
        (
            "app/counter.tsx",
            r#""use client";
export function Counter() { return null; }"#,
        ),
    ]);
    let page = page(&g);
    assert_eq!(
        links(&page.record),
        [
            "call",  // Tree, once:
            "child", // its recursive call stays a value the runtime renders
            "child", // and `n > 0` itself, rendered when it's falsy
            "child", // a component that calls a hook: React renders it
            "child", // async: not linked yet
            "child", // a JSX prop: a slot, not linked yet
            "child", // JSX children: a slot, not linked yet
            "call",  // text and expression children are plain props
            "child", // Box's `{title}`
            "child", // Box's `{children}`
            "child", // a parameter shadows the module's `Shadowed`
            "child", // a client component
        ]
    );
}

#[test]
fn falls_back_when_the_callee_cannot_be_lowered() {
    let g = graph(&[(
        "app/page.tsx",
        r"function Broken() { return <br>child</br>; }
export default function Page() { return <p>a<Broken />b</p>; }",
    )]);
    let page = page(&g);
    assert_eq!(links(&page.record), ["child"]);
    // Nothing of the failed inlining is left: the hole renders a string here.
    let out = rendered(&page, |w| {
        root_open(w, false, None);
        w.tag(tag::STRING).string("x");
        root_close(w, false);
    });
    assert_eq!(
        out,
        (
            "<p>a<!-- -->x<!-- -->b</p>".into(),
            "0:[\"$\",\"p\",null,{\"children\":[\"a\",\"x\",\"b\"]}]\n".into()
        )
    );
}

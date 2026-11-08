//! Linking (ADR-0012, section 5): child server components inlined into their caller's program,
//! rendered by the engine with input in the record's order, and compared with React's own output
//! for the same components (React DOM's `renderToString` and React's Flight server in production
//! mode; react 19.3.1).

// Tests may unwrap (docs/engineering/code-standards.md); this covers helpers outside #[test] fns.
#![allow(clippy::unwrap_used)]

mod common;

use common::Project;
use tsx2rs_compiler::graph::{ModuleGraph, ModuleId};
use tsx2rs_compiler::lower::{self, Body, Lowered, Slot, ValueKind};
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

/// Each component element and hole in a record, in order: `call` (inlined), `call N` (run with
/// template N), `slot` (a placed slot, then what's in it) or `child` (rendered at runtime).
fn links(record: &[Slot]) -> Vec<String> {
    let mut out = Vec::new();
    for slot in record {
        match slot {
            Slot::Call {
                body: Body::Inline(record),
                ..
            } => {
                out.push("call".to_owned());
                out.extend(links(record));
            }
            Slot::Call {
                body: Body::Template(t),
                ..
            } => out.push(format!("call {t}")),
            Slot::Placed { record, .. } => {
                out.push("slot".to_owned());
                out.extend(links(record));
            }
            Slot::Keyed { record, .. } => out.extend(links(record)),
            Slot::Island { source, record, .. } => {
                out.push(format!("island {source}"));
                out.extend(links(record));
            }
            Slot::Suspense {
                fallback, record, ..
            } => {
                out.push("suspense".to_owned());
                out.extend(links(fallback.as_deref().unwrap_or_default()));
                out.extend(links(record));
            }
            Slot::Value {
                kind: ValueKind::Child,
                ..
            } => out.push("child".to_owned()),
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
    let [
        Slot::RootWrap,
        Slot::Keyed {
            root: true,
            record: main,
            ..
        },
        Slot::RootWrap,
    ] = page.record.as_slice()
    else {
        panic!("{:?}", page.record);
    };
    let [call] = main.as_slice() else {
        panic!("{main:?}");
    };
    let Slot::Call {
        module,
        component,
        body: Body::Inline(record),
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
    // Wrap, the `<p>` with its key and `{amount}` and `{currency}`, wrap.
    let [
        Slot::RootWrap,
        Slot::Keyed { record: p, .. },
        Slot::RootWrap,
    ] = record.as_slice()
    else {
        panic!("{record:?}");
    };
    assert_eq!(p.len(), 2, "{p:?}");
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
function Hooked({ p }) { const v = use(p); return <i>{v}</i>; }
async function Later() { return <s>later</s>; }
function Shadowed() { return <u>never linked</u>; }
function Uses({ children }) { return <div>{children.length}</div>; }
function List({ items }) { return <ul>{items.map((i) => <li key={i}>{i}</li>)}</ul>; }
function Tree({ n, children }) { return <ul>{children}{n > 0 && <Tree n={n - 1}><b>x</b></Tree>}</ul>; }
export default function Page({ p, Shadowed }) {
  return (
    <main>
      <Hooked p={p} />
      <Later />
      <Shadowed />
      <Counter />
      <Uses><b>children used as a value</b></Uses>
      <List items={<b>a JSX prop it doesn't place</b>} />
      <Tree n={1}><i>slot</i></Tree>
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
            "child",                  // a component that calls a hook: React renders it
            "child", // async: its own Flight row (`$L1`), which comes with streaming
            "child", // a parameter shadows the module's `Shadowed`
            "island app/counter.tsx", // a client component (ADR-0020)
            "child", // `children` used as a value, not only placed
            "child", // a JSX prop the callee doesn't place
            "call",  // Tree, inlined
            "slot",  // with its slot, `<i>slot</i>`
            "child", // its recursive call has a slot, so it can't share a template
            "child", // and `n > 0` itself, rendered when it's falsy
        ]
    );
}

#[test]
fn places_slots_where_the_callee_places_them() {
    let g = graph(&[
        (
            "app/page.tsx",
            r#"import { Box } from "./box";
export default function Page({ t, y }) {
  return <section><Box title={<h2>{t}</h2>}><b key="k">x</b>{y}</Box></section>;
}"#,
        ),
        (
            "app/box.tsx",
            "export function Box({ children, title }) { return <div>{title}{children}</div>; }",
        ),
    ]);
    let page = page(&g);
    assert_eq!(
        links(&page.record),
        ["call", "slot", "child", "slot", "child"]
    );
    let [_, Slot::Keyed { record, .. }, _] = page.record.as_slice() else {
        panic!("{:?}", page.record);
    };
    let [Slot::Call { slots, .. }] = record.as_slice() else {
        panic!("{record:?}");
    };
    let names: Vec<&str> = slots.iter().map(|(p, _)| p.as_str()).collect();
    assert_eq!(names, ["title", "children"]);
    let out = rendered(&page, |w| {
        root_open(w, false, None);
        root_open(w, false, None); // Box's `<div>`
        w.tag(tag::STRING).string("T"); // the title slot's `{t}`
        w.tag(tag::STRING).string("y"); // the children slot's `{y}`
        root_close(w, false);
        root_close(w, false);
    });
    assert_eq!(
        out,
        (
            "<section><div><h2>T</h2><b>x</b>y</div></section>".into(),
            concat!(r#"0:["$","section",null,{"children":["$","div",null,{"children":[["$","h2",null,{"children":"T"}],[["$","b","k",{"children":"x"}],"y"]]}]}]"#, "\n").into()
        )
    );
}

#[test]
fn links_components_inside_slots_from_the_caller() {
    let g = graph(&[(
        "app/page.tsx",
        r#"function Card({ children }) { return <article><p>a</p>{children}z</article>; }
function Name({ name }) { return name; }
export default function Page() { return <Card>t<Name name="N" /></Card>; }"#,
    )]);
    let page = page(&g);
    assert_eq!(links(&page.record), ["call", "slot", "call", "child"]);
    let out = rendered(&page, |w| {
        root_open(w, false, None); // Card's `<article>`, the page's output
        w.tag(tag::STRING).string("N");
        root_close(w, false);
    });
    assert_eq!(
        out,
        (
            "<article><p>a</p>t<!-- -->N<!-- -->z</article>".into(),
            concat!(r#"0:["$","article",null,{"children":[["$","p",null,{"children":"a"}],["t","N"],"z"]}]"#, "\n").into()
        )
    );
}

#[test]
fn runs_recursive_components_with_call() {
    let g = graph(&[(
        "app/page.tsx",
        r"function Tree({ n }) { return <ul>{n > 0 && <Tree n={n - 1} />}</ul>; }
export default function Page() { return <main><Tree n={2} /></main>; }",
    )]);
    let page = page(&g);
    assert_eq!(links(&page.record), ["call", "call 1", "child"]);
    assert_eq!(page.program.templates().len(), 2);
    assert_eq!(links(&page.templates[0]), ["call 1", "child"]);
    let out = rendered(&page, |w| {
        root_open(w, false, None);
        root_open(w, false, None); // Tree, inlined
        w.condition(true);
        root_open(w, false, None); // Tree, template 1
        w.condition(true);
        root_open(w, false, None); // Tree, template 1 again
        w.condition(false).tag(tag::FALSE);
        root_close(w, false);
        root_close(w, false);
        root_close(w, false);
        root_close(w, false);
    });
    assert_eq!(
        out,
        (
            "<main><ul><ul><ul></ul></ul></ul></main>".into(),
            concat!(r#"0:["$","main",null,{"children":["$","ul",null,{"children":["$","ul",null,{"children":["$","ul",null,{"children":false}]}]}]}]"#, "\n").into()
        )
    );
}

#[test]
fn undoes_templates_made_by_a_failed_inlining() {
    // `Outer` fails to lower after `Tree` got a template: the template must go too, since it
    // uses chunks the undo drops.
    let g = graph(&[(
        "app/page.tsx",
        r"function Tree({ n }) { return <ol>{n > 0 && <Tree n={n - 1} />}</ol>; }
function Outer() { return <div><Tree n={1} /><br>child</br></div>; }
export default function Page() { return <main><Outer /><Tree n={1} /></main>; }",
    )]);
    let page = page(&g);
    assert_eq!(links(&page.record), ["child", "call", "call 1", "child"]);
    assert_eq!(page.program.templates().len(), 2);
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

#[test]
fn keeps_one_callers_slots_out_of_a_shared_template() {
    // The outer `Box` gets a slot; its recursive call doesn't, so template 1 (shared by every
    // call) reads `{children}` as a value.
    let g = graph(&[(
        "app/page.tsx",
        r"function Box({ n, children }) { return <div>{children}{n > 0 && <Box n={n - 1} />}</div>; }
export default function Page() { return <main><Box n={1}><b>s</b></Box></main>; }",
    )]);
    let page = page(&g);
    assert_eq!(links(&page.record), ["call", "slot", "call 1", "child"]);
    assert_eq!(links(&page.templates[0]), ["child", "call 1", "child"]);
}

#[test]
fn renders_client_components_as_islands_around_native_children() {
    let g = graph(&[
        (
            "app/page.tsx",
            r#"import Link from "next/link";
import { Price } from "./price";
export default function Page({ h, x }) {
  return <nav><Link href={h} prefetch={false} key="k">go <Price amount={x} /></Link><Link href="/" />!</nav>;
}"#,
        ),
        (
            "app/price.tsx",
            "export function Price({ amount }) { return <b>{amount}</b>; }",
        ),
    ]);
    let page = page(&g);
    let Slot::Keyed { record: nav, .. } = &page.record[1] else {
        panic!("{:?}", page.record);
    };
    assert_eq!(
        links(nav),
        ["island next/link", "call", "child", "island next/link"]
    );
    let Slot::Island {
        source,
        key,
        children: true,
        ..
    } = &nav[0]
    else {
        panic!("{nav:?}");
    };
    assert_eq!(source, "next/link");
    assert!(matches!(key, Some(tsx2rs_compiler::template::Key::Static(k)) if k == "k"));
    let (html, flight) = rendered(&page, |w| {
        root_open(w, false, None);
        // The first island: reference, key, props, HTML before, children, HTML after.
        w.tag(tag::REFERENCE).string("$L1");
        w.tag(tag::STRING).string("k");
        w.tag(tag::STRING).string("/p");
        w.tag(tag::FALSE);
        w.island(r#"<a href="/p">"#, 0);
        root_open(w, false, None); // Price's root
        w.tag(tag::STRING).string("9");
        root_close(w, false);
        w.island("</a>", 0);
        // The second: no children, so one `Island`.
        w.tag(tag::REFERENCE).string("$L1");
        w.tag(tag::NULL);
        w.tag(tag::STRING).string("/");
        w.island(r#"<a href="/"></a>"#, 0);
        root_close(w, false);
    });
    assert_eq!(
        html,
        r#"<nav><a href="/p">go <b>9</b></a><a href="/"></a>!</nav>"#
    );
    assert_eq!(
        flight,
        concat!(
            r#"0:["$","nav",null,{"children":[["$","$L1","k",{"href":"/p","prefetch":false,"#,
            r#""children":["go ",["$","b",null,{"children":"9"}]]}],"#,
            r#"["$","$L1",null,{"href":"/"}],"!"]}]"#,
            "\n"
        )
    );
}

#[test]
fn leaves_client_components_it_cannot_make_islands_to_react() {
    let g = graph(&[(
        "app/page.tsx",
        r#"import Link from "next/link";
import Image from "next/image";
export function Root({ h }) { return <Link href={h} />; }
export default function Page({ h, p }) {
  return (
    <div>
      <Link {...p} />
      <Link {...p}>x</Link>
      <Link href={h} icon={<i />} />
      <Link href={h} children="c" />
      <Link href={h} href="/" />
      <Image src={h} />
      <Root h={h} />
    </div>
  );
}"#,
    )]);
    let page = page(&g);
    let Slot::Keyed { record: div, .. } = &page.record[1] else {
        panic!("{:?}", page.record);
    };
    // A spread: an island, but with JSX children React's; a JSX prop, a `children` prop, a
    // prop twice: React; `Image`: an island; a client component at a component's root: React.
    assert_eq!(
        links(div),
        [
            "island next/link",
            "child",
            "child",
            "child",
            "child",
            "island next/image",
            "call",
            "child"
        ]
    );
}

#[test]
fn writes_an_islands_spread_props_as_one_object() {
    let g = graph(&[(
        "app/page.tsx",
        r#"import Image from "next/image";
export default function Page({ p }) {
  return <div><Image className="c" {...p} /></div>;
}"#,
    )]);
    let page = page(&g);
    let (html, flight) = rendered(&page, |w| {
        root_open(w, false, None);
        w.tag(tag::REFERENCE).string("$L2");
        w.tag(tag::NULL);
        // The props object as JSX builds it, `key` taken out.
        w.tag(tag::OBJECT).count(2);
        w.string("className").tag(tag::STRING).string("c");
        w.string("src").tag(tag::STRING).string("/a.png");
        w.island(r#"<img class="c" src="/a.png"/>"#, 0);
        root_close(w, false);
    });
    assert_eq!(html, r#"<div><img class="c" src="/a.png"/></div>"#);
    assert_eq!(
        flight,
        concat!(
            r#"0:["$","div",null,{"children":["$","$L2",null,{"className":"c","src":"/a.png"}]}]"#,
            "\n"
        )
    );
}

#[test]
fn renders_a_suspense_boundary_resolved_or_pending() {
    use tsx2rs_ir::input::html_output;
    let g = graph(&[(
        "app/page.tsx",
        r#"import { Suspense } from "react";
export default function Page({ x }) {
  return <main><Suspense fallback={<i>wait</i>}><b>{x}</b></Suspense>!</main>;
}"#,
    )]);
    let page = page(&g);
    let Slot::Keyed { record: main, .. } = &page.record[1] else {
        panic!("{:?}", page.record);
    };
    assert!(
        matches!(
            main.as_slice(),
            [Slot::Suspense {
                fallback: Some(_),
                ..
            }]
        ),
        "{main:?}"
    );
    let render = |pending: bool| {
        let mut w = InputWriter::new();
        root_open(&mut w, false, None);
        w.tag(tag::REFERENCE).string("$b");
        w.tag(tag::NULL);
        if pending {
            w.island(r#"<!--$?--><template id="B:1"></template>"#, 0);
            w.html_output(html_output::INLINE);
            w.island("<!--/$-->", 0);
            w.html_output(html_output::SEGMENTS);
            w.island(r#"<div hidden id="S:1">"#, 0);
        } else {
            w.island("<!--$-->", 0);
            w.html_output(html_output::NONE);
            w.island("", 0);
            w.html_output(html_output::INLINE);
            w.island("", 0);
        }
        w.tag(tag::STRING).string("x");
        w.island(if pending { "</div>" } else { "<!--/$-->" }, 0);
        root_close(&mut w, false);
        let mut output = Output::new();
        render(&page.program, &w.finish(), &mut output).unwrap();
        (
            String::from_utf8(output.html).unwrap(),
            String::from_utf8(output.segments).unwrap(),
            String::from_utf8(output.flight).unwrap(),
        )
    };
    let flight = r#"["$","main",null,{"children":[["$","$b",null,{"fallback":["$","i",null,{"children":"wait"}],"children":["$","b",null,{"children":"x"}]}],"!"]}]"#;
    assert_eq!(
        render(false),
        (
            "<main><!--$--><b>x</b><!--/$-->!</main>".into(),
            String::new(),
            flight.into()
        )
    );
    assert_eq!(
        render(true),
        (
            r#"<main><!--$?--><template id="B:1"></template><i>wait</i><!--/$-->!</main>"#.into(),
            r#"<div hidden id="S:1"><b>x</b></div>"#.into(),
            flight.into()
        )
    );
}

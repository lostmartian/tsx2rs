//! Lowering (ADR-0012): components compiled to programs, rendered by the engine with input in
//! their record's order, and compared with React's own output for the same JSX (React DOM's
//! `renderToString` and React's Flight server, rendering the component as an unkeyed function
//! component, as Next.js does; react 19.3.1).

// Tests may unwrap (docs/engineering/code-standards.md); this covers helpers outside #[test] fns.
#![allow(clippy::unwrap_used)]

mod common;

use common::Project;
use tsx2rs_compiler::lower::{self, Lowered, Slot};
use tsx2rs_engine::{Output, render};
use tsx2rs_ir::input::{InputWriter, tag};

/// The module's default-export component, lowered.
fn lowered(source: &str) -> Lowered {
    let p = Project::new(&[
        ("app/page.tsx", source),
        ("app/parts.tsx", "export const Price = () => null;"),
    ]);
    let graph = p.graph("app/page.tsx");
    assert!(graph.diagnostics.is_empty(), "{:?}", graph.diagnostics);
    let component = graph.modules[0]
        .components
        .iter()
        .find(|c| c.default_export)
        .unwrap();
    lower::component(component).unwrap()
}

/// Renders the program with the input `write` makes: HTML and the Flight row React writes.
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

/// The root element's key-path inputs (ADR-0008) for an unkeyed component: not wrapped, no key.
fn root_open(w: &mut InputWriter) {
    w.condition(false).tag(tag::NULL);
}

fn root_close(w: &mut InputWriter) {
    w.condition(false);
}

#[test]
fn folds_a_static_component_into_static_chunks() {
    let card = lowered(
        r#"export default () => <div className="card"><h1>Hello</h1>text{"more"}{42}</div>;"#,
    );
    // The root element's key is the runtime's (ADR-0008), its static props folded.
    let [
        Slot::RootWrap,
        Slot::Keyed {
            root: true,
            key: None,
            record,
            ..
        },
        Slot::RootWrap,
    ] = card.record.as_slice()
    else {
        panic!("{:?}", card.record);
    };
    assert_eq!(record, &[]);
    assert_eq!(
        rendered(&card, |w| {
            root_open(w);
            root_close(w);
        }),
        (
            r#"<div class="card"><h1>Hello</h1>text<!-- -->more<!-- -->42</div>"#.into(),
            "0:[\"$\",\"div\",null,{\"className\":\"card\",\"children\":[[\"$\",\"h1\",null,{\"children\":\"Hello\"}],\"text\",\"more\",42]}]\n".into()
        )
    );
}

#[test]
fn lowers_conditionals_to_branches() {
    let p = lowered(
        r#"export default function P({ a }) {
  return <p>{a ? <b>y</b> : "n"}{a && <i>z</i>}{0 || <s>f</s>}</p>;
}"#,
    );
    let yes = rendered(&p, |w| {
        root_open(w);
        w.condition(true).condition(true).condition(false);
        root_close(w);
    });
    assert_eq!(
        yes,
        (
            "<p><b>y</b><i>z</i><s>f</s></p>".into(),
            "0:[\"$\",\"p\",null,{\"children\":[[\"$\",\"b\",null,{\"children\":\"y\"}],[\"$\",\"i\",null,{\"children\":\"z\"}],[\"$\",\"s\",null,{\"children\":\"f\"}]]}]\n".into()
        )
    );
    // `a && …` renders `a` itself when it's falsy: `false`, nothing in HTML.
    let no = rendered(&p, |w| {
        root_open(w);
        w.condition(false)
            .condition(false)
            .tag(tag::FALSE)
            .condition(false);
        root_close(w);
    });
    assert_eq!(
        no,
        (
            "<p>n<s>f</s></p>".into(),
            "0:[\"$\",\"p\",null,{\"children\":[\"n\",false,[\"$\",\"s\",null,{\"children\":\"f\"}]]}]\n".into()
        )
    );
}

#[test]
fn lowers_maps_to_loops_with_keys() {
    let list = lowered(
        "export default function L({ xs }) {\n  return <ul>{xs.map((x) => <li key={x.id}>{x.n}</li>)}</ul>;\n}",
    );
    let two = rendered(&list, |w| {
        root_open(w);
        w.count(2);
        w.tag(tag::STRING).string("a").tag(tag::STRING).string("A");
        w.tag(tag::STRING).string("$b").tag(tag::NUMBER).number(2.0);
        root_close(w);
    });
    assert_eq!(
        two,
        (
            "<ul><li>A</li><li>2</li></ul>".into(),
            "0:[\"$\",\"ul\",null,{\"children\":[[\"$\",\"li\",\"a\",{\"children\":\"A\"}],[\"$\",\"li\",\"$$b\",{\"children\":2}]]}]\n".into()
        )
    );
    let none = rendered(&list, |w| {
        root_open(w);
        w.count(0);
        root_close(w);
    });
    assert_eq!(
        none,
        (
            "<ul></ul>".into(),
            "0:[\"$\",\"ul\",null,{\"children\":[]}]\n".into()
        )
    );
}

#[test]
fn gives_static_list_items_a_condition_to_read() {
    let items = lowered(
        "export default function O({ xs }) {\n  return <ol>{xs.map(() => <li>x</li>)}</ol>;\n}",
    );
    assert_eq!(
        rendered(&items, |w| {
            root_open(w);
            w.count(2).condition(true).condition(true);
            root_close(w);
        }),
        (
            "<ol><li>x</li><li>x</li></ol>".into(),
            "0:[\"$\",\"ol\",null,{\"children\":[[\"$\",\"li\",null,{\"children\":\"x\"}],[\"$\",\"li\",null,{\"children\":\"x\"}]]}]\n".into()
        )
    );
}

#[test]
fn lowers_an_element_with_a_spread_through_one_props_object() {
    let link = lowered(
        r#"export default function A({ p, id }) { return <a href="/x" id={id} {...p}>t</a>; }"#,
    );
    let html = rendered(&link, |w| {
        root_open(w);
        w.count(4);
        w.string("href").tag(tag::STRING).string("/x");
        w.string("id").tag(tag::STRING).string("i");
        w.string("target").tag(tag::STRING).string("_blank");
        w.string("className").tag(tag::STRING).string("c");
        root_close(w);
    });
    assert_eq!(
        html,
        (
            r#"<a href="/x" id="i" target="_blank" class="c">t</a>"#.into(),
            "0:[\"$\",\"a\",null,{\"href\":\"/x\",\"id\":\"i\",\"target\":\"_blank\",\"className\":\"c\",\"children\":\"t\"}]\n".into()
        )
    );
}

#[test]
fn mixes_static_and_dynamic_props_fragments_void_and_custom_elements() {
    let section = lowered(
        r#"export default function S({ id, cls }) {
  return <section id="s" className={cls} hidden data-n={2} onClick={id}><>a{"b"}</><br /><my-el flag>c</my-el></section>;
}"#,
    );
    let html = rendered(&section, |w| {
        root_open(w);
        w.tag(tag::STRING).string("k"); // className, an Op::Prop
        w.tag(tag::STRING).string("x"); // onClick, Flight only
        root_close(w);
    });
    assert_eq!(
        html,
        (
            r#"<section id="s" class="k" hidden="" data-n="2">a<!-- -->b<br/><my-el flag="">c</my-el></section>"#.into(),
            "0:[\"$\",\"section\",null,{\"id\":\"s\",\"className\":\"k\",\"hidden\":true,\"data-n\":2,\"onClick\":\"x\",\"children\":[[\"a\",\"b\"],[\"$\",\"br\",null,{}],[\"$\",\"my-el\",null,{\"flag\":true,\"children\":\"c\"}]]}]\n".into()
        )
    );
}

#[test]
fn dispatches_on_which_return_ran() {
    let p = lowered(
        "export default function P({ p }) {\n  if (!p) return null;\n  return <p>{p}</p>;\n}",
    );
    // The first return: `null` renders nothing; Flight's row is `null`.
    assert_eq!(
        rendered(&p, |w| {
            w.condition(true);
        }),
        (String::new(), "0:null\n".into())
    );
    assert_eq!(
        rendered(&p, |w| {
            w.condition(false);
            root_open(w);
            w.tag(tag::STRING).string("x");
            root_close(w);
        }),
        (
            "<p>x</p>".into(),
            "0:[\"$\",\"p\",null,{\"children\":\"x\"}]\n".into()
        )
    );
}

#[test]
fn writes_the_root_key_path_the_runtime_computes() {
    // A component under a key path (ADR-0008): its root gets the key, and React's array wrap.
    let p = lowered("export default () => <div>x</div>;");
    assert_eq!(
        rendered(&p, |w| {
            w.condition(true)
                .tag(tag::STRING)
                .string("$k")
                .condition(true);
        }),
        (
            "<div>x</div>".into(),
            "0:[[\"$\",\"div\",\"$$k\",{\"children\":\"x\"}]]\n".into()
        )
    );
}

#[test]
fn separates_text_next_to_holes_at_runtime() {
    let p = lowered("export default function T({ x }) { return <p>a{x}b</p>; }");
    let with = |text: &str| {
        rendered(&p, |w| {
            root_open(w);
            w.tag(tag::STRING).string(text);
            root_close(w);
        })
        .0
    };
    assert_eq!(with("m"), "<p>a<!-- -->m<!-- -->b</p>");
    // An empty string renders nothing and keeps React's text state.
    assert_eq!(with(""), "<p>a<!-- -->b</p>");
}

#[test]
fn renders_the_tested_value_unless_nullish() {
    let p = lowered("export default function N({ v }) { return <p>{v ?? <i>d</i>}</p>; }");
    assert_eq!(
        rendered(&p, |w| {
            root_open(w);
            w.condition(true).tag(tag::NUMBER).number(0.0);
            root_close(w);
        })
        .0,
        "<p>0</p>"
    );
    assert_eq!(
        rendered(&p, |w| {
            root_open(w);
            w.condition(false);
            root_close(w);
        })
        .0,
        "<p><i>d</i></p>"
    );
}

#[test]
fn reads_components_and_element_values_as_children() {
    // Element values are children by design (ADR-0012).
    let p = lowered(
        "import { Price } from './parts';\nexport default function C({ src }) { return <div><Price /><img src={src} /></div>; }",
    );
    // Unlinked (`lower::component`), a component's element is a child the runtime renders.
    let [_, Slot::Keyed { record, .. }, _] = p.record.as_slice() else {
        panic!("root wrap, the `<div>`, wrap: {:?}", p.record);
    };
    assert!(
        matches!(record.as_slice(), [Slot::Value { .. }, Slot::Value { .. }]),
        "two children: {record:?}"
    );
}

#[test]
fn refuses_what_react_rejects() {
    let p = Project::new(&[("app/page.tsx", "export default () => <br>x</br>;")]);
    let graph = p.graph("app/page.tsx");
    let component = &graph.modules[0].components[0];
    assert!(matches!(
        lower::component(component),
        Err(lower::LowerError::Fold { .. })
    ));
}

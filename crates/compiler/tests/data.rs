//! Data functions (ADR-0012, section 4): generated from a component's own source, and run in
//! Node.js to check the records they build.

// Tests may unwrap (docs/engineering/code-standards.md); this covers helpers outside #[test] fns.
#![allow(clippy::unwrap_used)]

mod common;

use std::fmt::Write as _;
use std::process::Command;

use common::Project;
use tsx2rs_compiler::graph::ModuleId;
use tsx2rs_compiler::{data, lower};

/// The data functions of `source`'s components (a module `app/page.jsx`).
fn functions(source: &str) -> String {
    generated(source).0
}

/// The data functions, and a runnable module: `source` with each component replaced by an empty
/// stub (Node.js can't parse JSX), then the data functions, which share its module scope as they
/// do in the product.
fn generated(source: &str) -> (String, String) {
    let p = Project::new(&[("app/page.jsx", source)]);
    let graph = p.graph("app/page.jsx");
    assert!(graph.diagnostics.is_empty(), "{:?}", graph.diagnostics);
    let module = &graph.modules[0];
    let lowered: Vec<_> = (0..module.components.len())
        .map(|i| lower::linked(&graph, ModuleId(0), i).unwrap())
        .collect();
    let pairs: Vec<_> = module
        .components
        .iter()
        .zip(&lowered)
        .map(|(c, l)| (c, l.record.as_slice()))
        .collect();
    let functions = data::functions(source, &pairs);
    let mut runnable = String::new();
    let mut at = 0;
    for (i, component) in module.components.iter().enumerate() {
        runnable.push_str(&source[at..component.span.0 as usize]);
        let _ = write!(runnable, "function __stub_{i}() {{}}");
        at = component.span.1 as usize;
    }
    runnable.push_str(&source[at..]);
    runnable.push_str(&functions);
    (functions, runnable)
}

/// Runs `module` (data functions, plain JavaScript) in Node.js, calls `call`, and returns the
/// record as JSON (`undefined` written as `"<undefined>"`).
fn run(module: &str, call: &str) -> String {
    let dir = std::env::temp_dir().join(format!(
        "tsx2rs-data-{}-{}",
        std::process::id(),
        std::thread::current()
            .name()
            .unwrap_or("t")
            .replace("::", "-")
    ));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("data.mjs"), module).unwrap();
    let script = format!(
        "import * as m from './data.mjs';\n\
         const show = (_, v) => (v === undefined ? '<undefined>' : v);\n\
         console.log(JSON.stringify({call}, show));\n"
    );
    std::fs::write(dir.join("run.mjs"), script).unwrap();
    let output = Command::new("node")
        .arg("run.mjs")
        .current_dir(&dir)
        .output()
        .unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

#[test]
fn rewrites_only_the_returned_templates() {
    let source = r"const fmt = (n) => `$${n.toFixed(2)}`;
export default function Price({ amount, className }) {
  // setup code, kept byte for byte
  const text = fmt(amount);
  return <p className={className}>{text}<span>USD</span></p>;
}
";
    insta::assert_snapshot!(functions(source));
}

#[test]
fn builds_records_in_program_order() {
    let source = r"const fmt = (n) => `$${n.toFixed(2)}`;
export default function Price({ amount, className }) {
  const text = fmt(amount);
  return <p className={className}>{text}<span>USD</span></p>;
}
";
    let (_, module) = generated(source);
    assert_eq!(
        run(
            &module,
            "m.__tsx2rs_data_Price({ amount: 2, className: 'x' })"
        ),
        r#"[[null,"x","$2.00"]]"#
    );
}

#[test]
fn evaluates_each_branch_test_once_and_renders_its_value() {
    let source = r#"let calls = 0;
const count = (v) => { calls++; return v; };
export function C({ a, v }) {
  return <p>{count(a) && <b>{a}</b>}{v ?? <i>d</i>}{a ? "y" : <s>n</s>}</p>;
}
export const calls_made = () => calls;
"#;
    let (functions, module) = generated(source);
    insta::assert_snapshot!(functions);
    // `count(a)` runs once; falsy, its value is the record's child.
    assert_eq!(
        run(&module, "m.__tsx2rs_data_C({ a: 0, v: null })"),
        r"[[null,[false,[0]],[false,[]],[false,[]]]]"
    );
    assert_eq!(
        run(
            &module,
            "(m.__tsx2rs_data_C({ a: 1, v: 1 }), m.calls_made())"
        ),
        "1",
        "the test of `count(a) && …` runs once"
    );
    assert_eq!(
        run(&module, "m.__tsx2rs_data_C({ a: 'k', v: 0 })"),
        r#"[[null,[true,["k"]],[true,[0]],[true,[]]]]"#
    );
}

#[test]
fn keeps_the_components_own_map_calls() {
    let source = r"export default function L({ xs }) {
  return (
    <ul>
      {xs.filter((x) => x.ok).map((x, i) => <li key={x.id}>{i}:{x.name}</li>)}
      {xs.map(() => <hr />)}
    </ul>
  );
}
";
    let (functions, module) = generated(source);
    insta::assert_snapshot!(functions);
    // Keys as React makes them: `undefined` is no key, anything else `"" + key`.
    assert_eq!(
        run(
            &module,
            "m.__tsx2rs_data_L({ xs: [{ id: 1, ok: true, name: 'a' }, { ok: true, name: 'b' }, { id: 3, ok: false }] })"
        ),
        r#"[[null,[[["1",0,"a"]],[[null,1,"b"]]],[[],[],[]]]]"#
    );
}

#[test]
fn indexes_several_returns_and_builds_spread_props() {
    let source = r#"export default function P({ p, rest }) {
  if (!p) return null;
  return <a href="/x" data-n={2} id={p} {...rest}>t</a>;
}
"#;
    let module = functions(source);
    assert_eq!(run(&module, "m.__tsx2rs_data_P({})"), "[[0,[]]]");
    assert_eq!(
        run(
            &module,
            "m.__tsx2rs_data_P({ p: 'i', rest: { target: '_blank', id: 'over' } })"
        ),
        r#"[[1,[[null,{"href":"/x","data-n":2,"id":"over","target":"_blank"}]]]]"#
    );
}

#[test]
fn writes_a_linked_child_as_a_pending_call() {
    // Evaluation order as Next.js's SWC compiles it: a key after a spread goes inside the props
    // (`createElement(C, {..., key: k(), ...})`); otherwise it's `jsx(C, {...}, k())`, after them.
    let source = r#"let order = [];
const log = (name, v) => { order.push(name); return v; };
function Label({ text, n, children }) {
  log("Label");
  return <b>{text}{n}{children}</b>;
}
export default function Page({ t, k, rest }) {
  return (
    <p>
      <Label text={log("text", t)} {...rest} key={log("key", k)} n={2}>a {log("child", t)}</Label>
      <Label key={log("key2", k)} text={log("text2", t)} />
      <Label>only</Label>
      {log("after", "!")}
    </p>
  );
}
export const order_seen = () => order;
"#;
    let (functions, module) = generated(source);
    insta::assert_snapshot!(functions);
    // `Label` itself hasn't run: the runtime calls it once the record is complete, as React
    // calls children after their parent.
    assert_eq!(
        run(
            &module,
            "[m.__tsx2rs_data_Page({ t: 'T', k: 7, rest: { n: 1, x: true } }), m.order_seen()]"
        ),
        concat!(
            r#"[[[null,[{"text":"T","n":2,"x":true,"key":7,"children":["a ","T"]},"<undefined>"],"#,
            r#"[{"text":"T"},7],[{"children":"only"},"<undefined>"],"!"]],"#,
            r#"["text","key","child","text2","key2","after"]]"#
        )
    );
}

#[test]
fn builds_slot_records_into_the_pending_call() {
    let source = r#"let order = [];
const log = (name, v) => { order.push(name); return v; };
function Box({ children, title }) {
  return <div>{title}{children}</div>;
}
export default function Page({ t, c }) {
  return <Box title={<h2>{log("title", t)}</h2>}>{log("child", c)}<b>static</b></Box>;
}
export const order_seen = () => order;
"#;
    let (functions, module) = generated(source);
    insta::assert_snapshot!(functions);
    // The caller builds each slot's record into the call's props, in JSX order (children last);
    // the callee's data function places those records as its values.
    assert_eq!(
        run(
            &module,
            "(() => { const [call] = m.__tsx2rs_data_Page({ t: 'T', c: 'C' }); \
             return [call, m.__tsx2rs_data_Box(call[0]), m.order_seen()]; })()"
        ),
        concat!(
            r#"[[{"title":["T"],"children":["C"]},"<undefined>"],"#,
            r#"[[null,["T"],["C"]]],["title","child"]]"#
        )
    );
}

//! The generated encoder (ADR-0016): its JavaScript for a page that uses every slot kind. What it
//! writes is checked end to end by the component fixtures (`tests/differential`).

// Tests may unwrap (docs/engineering/code-standards.md); this covers helpers outside #[test] fns.
#![allow(clippy::unwrap_used)]

mod common;

use common::Project;
use tsx2rs_compiler::encoder;
use tsx2rs_compiler::graph::ModuleId;
use tsx2rs_compiler::lower;

#[test]
fn writes_straight_line_code_per_template() {
    let p = Project::new(&[(
        "app/page.tsx",
        r#"import Link from "next/link";
import { Suspense } from "react";
function Tree({ n }) { return <ol>{n > 0 && <Tree n={n - 1} />}</ol>; }
function Card({ title, children }) { return <article>{title}{children}</article>; }
export default function Page({ a, xs, rest, s }) {
  if (!a) return null;
  return (
    <main className={a} style={s}>
      {a ? <b key="k">y</b> : "n"}
      {xs.map((x) => <li key={x}>{x}</li>)}
      {xs.map(() => <hr />)}
      <p {...rest} onClick={undefined}>spread</p>
      <Card title={<h2>{a}</h2>}>{a}</Card>
      <Tree n={2} />
      <Link href={a} prefetch={false}>{a}<i>x</i></Link>
      <Link href="/" key={a} />
      <Link {...rest} />
      <Suspense fallback={<hr />}><b>{a}</b></Suspense>
    </main>
  );
}"#,
    )]);
    let graph = p.graph("app/page.tsx");
    assert!(graph.diagnostics.is_empty(), "{:?}", graph.diagnostics);
    let page = graph.modules[0]
        .components
        .iter()
        .position(|c| c.default_export)
        .unwrap();
    let lowered = lower::linked(&graph, ModuleId(0), page).unwrap();
    let encoder = encoder::generate(&lowered);
    let callees: Vec<_> = encoder
        .callees
        .iter()
        .map(|(m, i)| graph.modules[m.0].components[*i].name.clone().unwrap())
        .collect();
    assert_eq!(callees, ["Card", "Tree"]);
    let islands: Vec<_> = encoder.islands.iter().map(|(s, _)| s.as_str()).collect();
    assert_eq!(islands, ["next/link"]);
    insta::assert_snapshot!(encoder.source);
}

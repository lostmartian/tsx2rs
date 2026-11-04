//! Template trees (ADR-0012, section 2): components, their template positions, and the static
//! parts and holes in each, printed compactly for snapshots.

// Tests may unwrap (docs/engineering/code-standards.md); this covers helpers outside #[test] fns.
#![allow(clippy::unwrap_used)]

mod common;

use std::fmt::Write as _;

use common::Project;
use tsx2rs_compiler::template::{
    Component, ComponentProp, Condition, Key, Literal, Node, Prop, Template,
};

/// The templates of `source` (a module `app/page.tsx`), printed with each hole's source text.
/// `app/parts.tsx` exists for imports to resolve.
fn templates(source: &str) -> String {
    let p = Project::new(&[
        ("app/page.tsx", source),
        (
            "app/parts.tsx",
            "export const Price = () => null; export const ui = {};",
        ),
    ]);
    let graph = p.graph("app/page.tsx");
    let module = &graph.modules[0];
    let mut out = String::new();
    for diagnostic in &graph.diagnostics {
        let _ = writeln!(out, "! {} {}", diagnostic.code, diagnostic.message);
    }
    for component in &module.components {
        print_component(&mut out, source, component);
    }
    out
}

fn print_component(out: &mut String, source: &str, c: &Component) {
    let _ = writeln!(
        out,
        "component {}{}{}{}",
        c.name.as_deref().unwrap_or("(anonymous)"),
        if c.default_export { " default" } else { "" },
        if c.is_async { " async" } else { "" },
        if c.uses_hooks { " hooks" } else { "" },
    );
    for (i, t) in c.returns.iter().enumerate() {
        let _ = writeln!(out, "  return {i}:");
        print_node(out, source, &t.root, 2);
    }
}

fn text(source: &str, span: (u32, u32)) -> &str {
    &source[span.0 as usize..span.1 as usize]
}

fn print_templates(out: &mut String, source: &str, templates: &[Template], depth: usize) {
    for (i, t) in templates.iter().enumerate() {
        let _ = writeln!(out, "{}return {i}:", "  ".repeat(depth));
        print_node(out, source, &t.root, depth + 1);
    }
}

fn print_props(source: &str, props: &[Prop]) -> String {
    props
        .iter()
        .map(|p| match p {
            Prop::Static { name, json } => format!(" {name}={json}"),
            Prop::Dynamic { name, value } => format!(" {name}={{{}}}", text(source, *value)),
            Prop::Spread(span) => format!(" {{...{}}}", text(source, *span)),
        })
        .collect()
}

fn print_key(source: &str, key: Option<&Key>) -> String {
    match key {
        None => String::new(),
        Some(Key::Static(k)) => format!(" key={k:?}"),
        Some(Key::Dynamic(span)) => format!(" key={{{}}}", text(source, *span)),
    }
}

fn print_node(out: &mut String, source: &str, node: &Node, depth: usize) {
    let pad = "  ".repeat(depth);
    match node {
        Node::Element(e) => {
            let _ = writeln!(
                out,
                "{pad}<{}{}{}>",
                e.tag,
                print_key(source, e.key.as_ref()),
                print_props(source, &e.props)
            );
            for child in &e.children {
                print_node(out, source, child, depth + 1);
            }
        }
        Node::Text(t) => {
            let _ = writeln!(out, "{pad}text {t:?}");
        }
        Node::Number(n) => {
            let _ = writeln!(out, "{pad}number {n}");
        }
        Node::Literal(Literal::Null) => {
            let _ = writeln!(out, "{pad}null");
        }
        Node::Literal(Literal::Bool(b)) => {
            let _ = writeln!(out, "{pad}{b}");
        }
        Node::Fragment(children) => {
            let _ = writeln!(out, "{pad}<>");
            for child in children {
                print_node(out, source, child, depth + 1);
            }
        }
        Node::Hole(span) => {
            let _ = writeln!(out, "{pad}hole {{{}}}", text(source, *span));
        }
        Node::Conditional(c) => {
            let op = match c.condition {
                Condition::Ternary => "?:",
                Condition::And => "&&",
                Condition::Or => "||",
                Condition::Nullish => "??",
            };
            let _ = writeln!(out, "{pad}{op} {{{}}}", text(source, c.test));
            print_node(out, source, &c.consequent, depth + 1);
            if let Some(alternate) = &c.alternate {
                let _ = writeln!(out, "{pad}else");
                print_node(out, source, alternate, depth + 1);
            }
        }
        Node::List(l) => {
            let _ = writeln!(out, "{pad}map {{{}}}", text(source, l.array));
            print_templates(out, source, &l.item, depth + 1);
        }
        Node::Component(c) => {
            let name = match &c.member {
                Some(member) => format!("{}.{member}", c.name),
                None => c.name.clone(),
            };
            let mut props = String::new();
            let mut slots = Vec::new();
            for p in &c.props {
                match p {
                    ComponentProp::Value(p) => {
                        props.push_str(&print_props(source, std::slice::from_ref(p)));
                    }
                    ComponentProp::Slot { name, template } => slots.push((name, template)),
                }
            }
            let _ = writeln!(
                out,
                "{pad}component <{name}{}{props}>",
                print_key(source, c.key.as_ref())
            );
            for (name, template) in slots {
                let _ = writeln!(out, "{pad}  slot {name}:");
                print_node(out, source, template, depth + 2);
            }
            for child in &c.children {
                print_node(out, source, child, depth + 1);
            }
        }
        Node::ElementValue(v) => {
            let _ = writeln!(
                out,
                "{pad}element value ({:?}) {{{}}}",
                v.reason,
                text(source, v.span)
            );
        }
    }
}

#[test]
fn finds_components_of_every_kind() {
    insta::assert_snapshot!(templates(
        r"
function Plain() { return <p>a</p>; }
export function Named() { return <p>b</p>; }
export default async function Page() { return <p>c</p>; }
const Arrow = () => <p>d</p>;
export const Exported = async () => { return <p>e</p>; };
const Expr = function () { return <p>f</p>; };
function helper() { return <p>not a component</p>; }
const lower = () => <p>not a component either</p>;
"
    ));
}

#[test]
fn builds_elements_with_static_dynamic_and_spread_props() {
    insta::assert_snapshot!(templates(
        r#"
export default function Page({ id, rest, title }) {
  return (
    <section key="s" className="card" id={id} hidden data-n={2} {...rest} title={`${title}!`}>
      <h1 key={id}>Hello, world</h1>
      {"literal"}{42}{-1}{null}{true}
      {title}
    </section>
  );
}
"#
    ));
}

#[test]
fn applies_jsx_whitespace_rules() {
    insta::assert_snapshot!(templates(
        "export default () => (\n  <div>\n    first line\n    second line\n    <b> spaced </b>\n    {/* comment */}\n  </div>\n);\n"
    ));
}

#[test]
fn turns_conditionals_with_jsx_into_branches_and_others_into_holes() {
    insta::assert_snapshot!(templates(
        r#"
export default function Page({ a, b, c, d }) {
  return (
    <div>
      {a ? <b>yes</b> : <i>no</i>}
      {a ? <b>yes</b> : null}
      {b && <span>shown</span>}
      {c || <span>fallback</span>}
      {d ?? <span>default</span>}
      {a ? "x" : "y"}
      {a && b}
    </div>
  );
}
"#
    ));
}

#[test]
fn turns_maps_returning_jsx_into_lists() {
    insta::assert_snapshot!(templates(
        r"
export default function Page({ items, names }) {
  return (
    <ul>
      {items.map((item) => <li key={item.id}>{item.name}</li>)}
      {items.filter((i) => i.ok).map(function (item) {
        if (!item.name) return null;
        return <li key={item.id}>{item.name}</li>;
      })}
      {names.map(String)}
    </ul>
  );
}
"
    ));
}

#[test]
fn records_components_with_props_slots_and_children() {
    insta::assert_snapshot!(templates(
        r#"
import { Price, ui } from "./parts";
export default function Page({ amount, rest }) {
  return (
    <div>
      <Price key="p" amount={amount} currency="USD" {...rest} icon={<svg><path d="M0" /></svg>} />
      <ui.Card>
        <h2>Title</h2>
        {amount}
      </ui.Card>
    </div>
  );
}
"#
    ));
}

#[test]
fn keeps_hoistable_special_and_context_elements_as_element_values() {
    insta::assert_snapshot!(templates(
        r#"
export default function Page({ src, json, label }) {
  return (
    <div>
      <img src={src} alt="" />
      <title>Static title</title>
      <script type="application/ld+json" dangerouslySetInnerHTML={{ __html: json }} />
      <input value={label} />
      <svg viewBox="0 0 1 1"><path d="M0" /></svg>
      <svg><title>{label}</title></svg>
      <div dangerouslySetInnerHTML={{ __html: json }} />
      <span children={label} />
    </div>
  );
}
"#
    ));
}

#[test]
fn makes_one_template_per_return_and_skips_nested_functions() {
    insta::assert_snapshot!(templates(
        r#"
import { notFound } from "next/navigation";
export default async function Page({ product, items }) {
  if (!product) return notFound();
  const render = () => <p>inner</p>;
  if (product.hidden) {
    return null;
  }
  items.forEach((item) => { return <b>also inner</b>; });
  return <main>{product.title}</main>;
}
"#
    ));
}

#[test]
fn flags_components_that_call_hooks() {
    insta::assert_snapshot!(templates(
        r#"
import { use, useId } from "react";
export function WithUse({ promise }) { const value = use(promise); return <p>{value}</p>; }
export function WithId() { const id = useId(); return <p id={id} />; }
export function NotAHook({ user }) { const x = user(); return <p>{x}</p>; }
"#
    ));
}

#[test]
fn reports_jsx_it_cannot_compile() {
    insta::assert_snapshot!(templates(
        r"
export default function Page({ items }) {
  return <div>{...items}</div>;
}
"
    ));
}

# Differential tests: the engine against React

Layer 4 of the [testing strategy](../../docs/engineering/testing.md): **React is the oracle for the engine.**

- **Generate:** a property test (`tests/react.rs`, proptest) generates element trees: generic tags, `a`, void tags, and nesting. **Props** cover every class of React DOM's `pushAttribute`: renamed (`className`, `htmlFor`, SVG and XML names), string, any-value, boolean, overloaded-boolean, numeric and URL props, `data-`/`aria-` names in any case, event handlers, internal props, unsafe and unknown names. Their values are any primitive, including numeric strings (`" 3 "`, `0x10`, `Infinity`) and `javascript:` URLs. Elements also get **`style`** props: objects whose entries cover camelCase and vendor-prefixed names (`Webkit`, `Moz`, `ms`), unitless properties, custom properties (`--x`), escaping and non-ASCII case mapping, with values of every primitive type (including whitespace to trim, `""`, `0` and `NaN`), or `null` / `undefined`. Each element's props are lowered either one by one (`Op::Prop` and `Op::Style`, names known at compile time) or as one `Op::Attributes` object (a spread, names known only at runtime). Children include text with `< > & " ' $`, Unicode, newlines and `""`, numbers including `NaN`, `±Infinity`, `-0` and `1e21`, bigints (beyond 64 bits too), `null`, `undefined`, booleans, and arrays of any of these, nested. Some children are **holes**: values the engine reads at runtime through `Op::Child` (ADR-0005), as a `{x}` child compiles.
- **Element values** (ADR-0007): some elements, and every element with rules of its own, are lowered as one `Op::Child` whose input is the whole element and its descendants. They get keys (including `$`, which Flight escapes) and sometimes `children` at any position among the props. The special elements are `input`, `img`, `source`, `textarea`, `select`, `option`, `optgroup`, `button`, `form`, `pre`, `listing`, `object`, `menuitem`, `noscript`, `picture`, `table`, `tr`, `math` and `font-face`, plus custom elements (`my-element`, also as templates). Their props favour the ones their rules treat specially (`value`, `defaultValue`, `checked`, `name`, the form-action props, `selected`, `src`, `srcSet`, `loading`, `fetchPriority`, `data`, …) and `dangerouslySetInnerHTML`. **`<select>` trees** put options directly, in `<optgroup>`s and other elements, and behind elements that reset the select (`<table>`, `<svg>`, `<noscript>`), with single or array values. Void elements sometimes get children.
- **Compiled components** (ADR-0008): some subtrees are lowered as their own template, reached either by `Op::Call` or by a template node in an `Op::Child` hole. React gets a real function component that returns the same subtree, so its rules for component elements apply (a component child makes a void element invalid), and the outputs must be identical.
- **Hoistable elements** (ADR-0009): `<title>`, `<meta>`, `<link>`, `<script>` and `<style>` with the props that decide whether React hoists them (`itemProp`, `rel`, `href`, `precedence`, `disabled`, `onLoad`, `src`, `async`, …), inside `<svg>`, `<noscript>`, `<math>`, `<foreignObject>` or a `<div>`, with raw text that needs React's `</script>` and `</style>` escaping; and `<html>`/`<head>`/`<body>` inside the body.
- **Errors:** where React DOM throws (content on a void element, `children` with `dangerouslySetInnerHTML`, a `<textarea>` with a value and children, …), the engine must return an error too. Where the engine declines an element it doesn't render yet (one React hoists, or an `<img>` it preloads), React must have hoisted something: rendered as the body of a document, its `<head>` isn't empty. The property test runs on a thread with a large stack, because shrinking a failure through these generators recurses deeply.
- **Lower:** each tree becomes a template program the way the compiler will lower host elements (`src/lower.rs`). Element structure becomes static chunks, and every value is read from the input, so the engine does all escaping and separating.
- **Render twice:** the engine renders the program. The same tree goes to `oracle/oracle.ts`, which renders it with **React DOM's server** (`renderToString`) and **React's Flight server** (`react-server-dom-webpack`, under the `react-server` condition).
- **Compare:** every tree is the body of a document (`<html><head></head><body>`) on both sides, and the whole document must be byte-identical (so hoisting into `<head>`, its order and deduplication are checked too), and the Flight must decode to the same tree, with the same resource hint rows (`:HL`, `:Hm`) in the same order (with the identity harness). A mismatch is shrunk to a minimal tree.

## Running

```bash
pnpm install                                   # installs React for the oracle
cargo nextest run -p tsx2rs_differential       # 512 trees
DIFFERENTIAL_CASES=20000 cargo nextest run -p tsx2rs_differential
```

CI runs 2,000 trees per PR in the "Differential (engine vs React)" job. The other Rust jobs exclude this crate because it needs Node.js packages.

## Not generated yet

These have React rules of their own, and each needs its own generator and engine support before the compiler may emit it:

- **Fragments, Suspense boundaries,** and client components (islands).

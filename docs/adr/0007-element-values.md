# ADR-0007: Element values are a node kind that `Child` reads

- **Status:** Accepted
- **Date:** 2026-10-26

## Context

ADR-0005 (section 3) renders a React element that reaches a hole at runtime (an *element value*)
natively: "a host element goes to the engine's generic element op". Its section 7 lists an
`Element` op ("a host element whose tag or props are known only at runtime") and the special
elements the engine must render by React DOM's rules, and it leaves the exact form of the new ops
to the pull request that adds them.

Three facts about React decide that form:

- **An element can be anywhere a node can be:** a `{x}` child, an item of an array, the `children`
  of another element. A hole doesn't know in advance whether it holds a string, an array or an
  element.
- **React DOM writes some props out of their order.** `<input>` writes `name`, the form-action
  props, `checked` and `value` after its other attributes; `<button>` and `<form>` move their
  form-action props to the end. React's Flight server writes every prop in the order of the props
  object. One pass over the props therefore can't produce both outputs in order, unless the
  engine sees the whole element.
- **Some output depends on ancestors.** An `<option>` is marked `selected` by comparing its value
  with the value of the nearest enclosing `<select>`, through any number of elements in between,
  and the context resets at `<svg>`, `<math>`, tables, `<noscript>` and `<picture>`
  (`getChildFormatContext`). `<noscript>` and `<picture>` also change whether an `<img>` gets a
  preload.

## Decision

### Element nodes, read by `Child`

There is no separate `Element` op. An element is a **node kind** (`tag::ELEMENT`) of the input
`Op::Child` already reads. So an element value can appear in any hole, array or `children`, and
nesting needs nothing extra.

```text
element   = ELEMENT, type: string, key: NULL | STRING,
            u32 count, count × entry, [children node]
entry     = name: string, value          (any prop but `children`)
          | "children"                   (no value: marks where `children` is in the props)
```

- **Props are in the props object's order,** which is the order Flight writes. The `children`
  entry carries no value; the node follows the other entries, so the engine can write the start
  tag's attributes before it reads any content. The engine writes the props that follow
  `children` to Flight after the children, from a second cursor over their (small) values.
- **Prop values:** a primitive for attributes; a style value for `style`; an object with `__html`
  (a primitive) for `dangerouslySetInnerHTML`; a primitive or an array of primitives for a
  `<select>`'s `value` and `defaultValue`. Anything else is a `BadTag`: the encoder sends such an
  element to React (ADR-0005, path 3).
- **The engine keeps the element's context** while it walks element nodes: the enclosing
  `<select>`'s value, and whether a `<noscript>` or `<picture>` encloses it. A `Child` hole
  starts in the document body's context, so a compiled template must not place a `Child` hole
  whose node could be an `<option>` inside a `<select>`. The compiler lowers such a `<select>` as
  an element value.

### The rules the engine applies

React DOM's server renderer (`pushStartInstance`, react-dom-server 19.3.1), for:

- **generic elements** (`pushStartGenericElement`) and `<a>` (`href=""` is written)
- **void elements** (`pushSelfClosing`): written `<tag …/>`, and `children` or
  `dangerouslySetInnerHTML` is an error
- **`<input>`, `<button>`, `<form>`:** their props written in React's order
- **`<select>` and `<option>`:** `selected` from the enclosing select's value, including arrays
  (`multiple`) and the option's text when it has no `value`
- **`<textarea>`:** its value (or its `children`) as escaped content, with React's leading newline
- **`<pre>` and `<listing>`:** the leading newline
- **`<object>`:** `data` as a URL, dropped when empty
- **`<menuitem>`:** no content
- **custom elements** (a `-` in the tag): `className` becomes `class`, `true` becomes `""`,
  `false` drops the attribute, and other names are written as given
  (`ElementRules::Custom`, also usable by `Prop` and `Attributes`)
- **`dangerouslySetInnerHTML`** and **tag names** checked as React checks them

Wherever React DOM throws, the engine returns `RenderError::InvalidValue`, and the request falls
back.

### What isn't rendered yet

Elements that React DOM hoists or writes somewhere else get
`RenderError::UnsupportedElement`, and the request falls back:

- `<title>`, `<meta>`, `<link>`, `<script>` and `<style>`, which can be hoisted into `<head>`
- an `<img>` that React would preload (a `<link rel="preload">` in `<head>`)
- `<html>`, `<head>` and `<body>`, which Next.js's wrapper provides (ADR-0005, section 9)

The hoistables pull request (ADR-0005, section 7) renders them. Until then, this costs fallbacks,
never wrong output. The engine doesn't track the SVG and `<noscript>` contexts in which React
renders some of these in place; it rejects them there too.

## Options considered

- **An `Element` op with the tag in the program** and the props read at runtime. It covers only
  elements whose type is known at build time, so an element inside an array or in another
  element's `children` still needs a node kind. Rejected: two ways to do one thing.
- **`children` inline among the props.** The engine would have to render the content before the
  start tag is complete, or buffer it. Rejected for the marker entry.
- **Buffering the start tag.** Allocates per element, and still needs the whole props object.
  Rejected.
- **Tracking every insertion mode** (SVG, MathML, tables, head). Nothing this pull request
  renders depends on them; the hoistables pull request adds what it needs.

## Consequences

- One input grammar for nodes: strings, numbers, bigints, booleans, `null`, `undefined`, arrays
  and elements. The generated encoder walks a React element's props once, in order.
- Special elements in compiled templates: the compiler lowers one whose props aren't all static
  as an element value, unless its props follow the generic rules (custom elements use
  `ElementRules::Custom` with `Prop` and `Attributes`).
- Rendering an element costs a second read of the props after `children` only when `children`
  isn't the last prop, which JSX never produces.
- Client components and compiled components inside element values come with `Call` and islands
  in values.

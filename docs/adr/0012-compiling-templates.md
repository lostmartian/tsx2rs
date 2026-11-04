# ADR-0012: Compiling templates: template trees, holes, folding through the engine

- **Status:** Accepted
- **Date:** 2026-11-04

## Context

ADR-0005 decided *what* the compiler does for server components: templates for JSX in template
positions, data functions computing the holes, linking with `Call` where inlining isn't possible.
The engine side is complete (ADR-0007 to ADR-0011). This ADR fixes *how* the compiler half is
built, so its pull requests (template extraction, lowering, data functions, linking, fixtures)
share one design.

One constraint shapes it more than any other. **ARCHITECTURE.md invariant 5: only the engine
turns data into HTML or Flight text.** A template's static parts (`<div class="card">`, fixed
text, fixed props) end up as precomputed chunks. If the compiler wrote them itself, React DOM's
escaping and attribute rules would exist twice, and could drift apart.

## Decision

### 1. Static markup is folded by the engine, at build time

The engine gets a build-time API, `tsx2rs_engine::fold`, and the compiler depends on the engine
for it. The engine still depends on nothing but `ir`.

- **`fold::node(input)`:** a fully static node, in `Child`'s input grammar (ADR-0007), rendered
  exactly as at runtime. It returns its HTML, its Flight, and whether its HTML starts and ends
  with a text node: the `text_start`/`text_end` flags of `Op::Static`, which decide React's
  `<!-- -->` separators next to holes.
- **`fold::props(rules, input)`:** a run of static props, in `Attributes`' input grammar,
  rendered through `Op::Attributes`. It returns the HTML attributes and the Flight
  `"name":value` pairs.
- **Both refuse what can't be a static chunk:** anything that hoists into `<head>` or emits a
  Flight hint. Those elements are always element values (ADR-0011).

So every static byte comes from the same code that renders at runtime, and its tests (unit,
differential against React) cover it.

### 2. Template trees

For each server component, the compiler builds a **template tree** for every JSX expression
in a template position (ADR-0005, section 3), in source order:

- **Nodes:**
  - **Host element:** tag, key, props, children. Every prop is *static* (a literal),
    *dynamic* (an expression) or a *spread*.
  - **Text:** JSX text, after React's JSX whitespace rules.
  - **Hole:** a `{expression}` whose value is known only at runtime.
  - **Conditional:** `c ? <A/> : <B/>`, `c && <A/>`, `c || <A/>`, `c ?? <A/>`.
  - **List:** `xs.map(x => <Item/>)`.
  - **Component:** `<Child …/>`, a pending call (ADR-0005, section 4).
  - **Element value:** JSX the compiler keeps as real JSX.
- **What becomes an element value:**
  - every element that hoists or hints (`title`, `meta`, `link`, `script`, `style`, `img`;
    ADR-0011)
  - every element with rules of its own (`input`, `select`, `option`, `textarea`, `button`,
    `form`, `object`, `pre`, `listing`, `menuitem`; ADR-0007)
  - every context-changing element with anything runtime inside (`svg`, `noscript`, `picture`,
    `math`, tables; ADR-0009)

  Everything else uses generic element rules and becomes template markup.
- **A component with several `return`s** (early returns, `if (!x) return null`) has one template
  per `return`. Its program dispatches on which one ran.

### 3. Holes and the ops they lower to

The data record holds one value per hole, in **program order**. That's the order the engine
reads input, which for JSX is source order: an element's props as written, then its children.

| Hole | Op | Data function writes |
|---|---|---|
| Dynamic prop, name known | `Prop` (name and kind from `ir::attribute::classify`) | the expression |
| `style={…}` | `Style` | the expression |
| Spread `{...props}` | `Attributes` | the object, as JSX builds it |
| `{x}` child | `Child` | the expression |
| Element key (dynamic) | `Value` (a string, or `null`) | `String(key)`, as React does |
| `c ? <A/> : <B/>` | `If`, with both branches | the truthiness of `c` |
| `c && <A/>` (and `\|\|`, `??`) | `If`, the template, and `Child` for the value React renders otherwise | the truthiness, then `c` |
| `xs.map(…)` | `Loop` over the item's template | each item's record, with its key |
| Which `return` ran | `If` chain | its index |
| Root key path and wrap (ADR-0008) | `Value`, `If` | computed by the runtime |

- **Static parts never become holes.** Literal props and text are folded into chunks, and a
  literal `{"text"}` child is folded as a text node.
- **The compiler never assumes a type.** A `{x}` is always `Child`, even when `x` looks like a
  string.

### 4. Data functions are generated JavaScript

- **Shape:** a data function is the component's own source, rewritten by spans (ADR-0005,
  section 4). Every returned JSX expression is replaced by its data record, an array literal of
  the hole expressions in program order.
- **Child components:** a child component's hole is a pending call (the component and its
  props object).
- **Everything else stays byte for byte:** setup code, imports and comments are untouched, so
  evaluation order and closures are the component's own.
- **Where they go:** the compiler emits them as JavaScript source next to the program. The
  bundler transform puts them into Next.js's bundle (step 7).

### 5. Linking

- **Inlined by default.** A server component used under a name known at build time is inlined
  into its caller's template. Its data still comes from its own data function, so evaluation
  order matches React's.
- **`Call` or template nodes** for recursion, for components known only at runtime, and above a
  size limit (ADR-0008).
- **`children` and other JSX props** are slots when the callee only places them (ADR-0005,
  section 6).

### 6. Testing

- **Each phase has snapshot tests** on small inputs and on Commerce's components.
- **Fixtures, the step's acceptance test:** each fixture is a small `.tsx` component, compiled,
  then its data function run in Node.js, its record encoded, and the program rendered by the
  engine. The result is compared with React rendering the original component: HTML byte for
  byte, Flight tree and hints equal.
- **Folding is property-tested** against runtime rendering: folding two nodes and joining them
  with the separator their flags call for must equal rendering them together.

## Options considered

- **The compiler writes static markup itself,** with its own copy of React DOM's rules.
  Rejected: invariant 5, and the rules would drift.
- **No static chunks:** every literal becomes a hole with constant input. That keeps one writer,
  but loses most of the speed (static chunks are what the speed gate measured). Rejected.
- **Folding inside `napi`** (which depends on both crates). It keeps `compiler` free of the
  engine, but the compiler's own tests couldn't produce programs. Rejected.

## Consequences

- `compiler` depends on `engine`: ARCHITECTURE.md's layering gains `engine` ← `compiler`, and
  the engine still never depends on the compiler.
- The engine grows a small build-time API, outside the request path.
- The encoder (step 6) is generated from the same hole list as the program, so the two can't
  disagree.

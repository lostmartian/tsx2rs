# Differential tests: the engine against React

Layer 4 of the [testing strategy](../../docs/engineering/testing.md): **React is the oracle for the engine.**

- **Generate:** a property test (`tests/react.rs`, proptest) generates element trees: generic tags, `a`, void tags, and nesting. **Props** cover every class of React DOM's `pushAttribute`: renamed (`className`, `htmlFor`, SVG and XML names), string, any-value, boolean, overloaded-boolean, numeric and URL props, `data-`/`aria-` names in any case, event handlers, internal props, unsafe and unknown names. Their values are any primitive, including numeric strings (`" 3 "`, `0x10`, `Infinity`) and `javascript:` URLs. Elements also get **`style`** props: objects whose entries cover camelCase and vendor-prefixed names (`Webkit`, `Moz`, `ms`), unitless properties, custom properties (`--x`), escaping and non-ASCII case mapping, with values of every primitive type (including whitespace to trim, `""`, `0` and `NaN`), or `null` / `undefined`. Each element's props are lowered either one by one (`Op::Prop` and `Op::Style`, names known at compile time) or as one `Op::Attributes` object (a spread, names known only at runtime). Children include text with `< > & " ' $`, Unicode, newlines and `""`, numbers including `NaN`, `±Infinity`, `-0` and `1e21`, bigints (beyond 64 bits too), `null`, `undefined`, booleans, and arrays of any of these, nested. Some children are **holes**: values the engine reads at runtime through `Op::Child` (ADR-0005), as a `{x}` child compiles.
- **Lower:** each tree becomes a template program the way the compiler will lower host elements (`src/lower.rs`). Element structure becomes static chunks, and every value is read from the input, so the engine does all escaping and separating.
- **Render twice:** the engine renders the program. The same tree goes to `oracle/oracle.ts`, which renders it with **React DOM's server** (`renderToString`) and **React's Flight server** (`react-server-dom-webpack`, under the `react-server` condition).
- **Compare:** the HTML must be byte-identical, and the Flight must decode to the same tree (with the identity harness). A mismatch is shrunk to a minimal tree.

## Running

```bash
pnpm install                                   # installs React for the oracle
cargo nextest run -p tsx2rs_differential       # 512 trees
DIFFERENTIAL_CASES=20000 cargo nextest run -p tsx2rs_differential
```

CI runs 2,000 trees per PR in the "Differential (engine vs React)" job. The other Rust jobs exclude this crate because it needs Node.js packages.

## Not generated yet

These have React rules of their own, and each needs its own generator and engine support before the compiler may emit it:

- **Elements with prop rules of their own:** `input`, `textarea`, `select`/`option`, `button`, `form`, `img`, `link`, `meta`, `script`, `style`, `title`, `noscript`, custom elements.
- **Keys, fragments, Suspense boundaries,** and client components (islands).

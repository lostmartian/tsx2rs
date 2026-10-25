# Differential tests: the engine against React

Layer 4 of the [testing strategy](../../docs/engineering/testing.md): **React is the oracle for the engine.**

- **Generate:** a property test (`tests/react.rs`, proptest) generates element trees: tags, void tags, string and boolean props (`className`, `id`, `title`, `data-x`, `hidden`, `disabled`, including `null` and `undefined`), and nesting. Children include text with `< > & " ' $`, Unicode, newlines and `""`, numbers including `NaN`, `±Infinity`, `-0` and `1e21`, `null`, and booleans.
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

- **URL props** (`href`, `src`, `action`, `formAction`): React 19 replaces `javascript:` URLs.
- **`style` objects:** hyphenation, `px` units, unitless properties.
- **Raw-text and special elements:** `script`, `style`, `title`, `textarea`, `select`/`option`, `noscript`.
- **Keys, fragments, Suspense boundaries,** and client components (islands).

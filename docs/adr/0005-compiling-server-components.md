# ADR-0005: Compiling server components: JavaScript computes values, templates render them

- **Status:** Proposed
- **Date:** 2026-10-25

## Context

Milestone 1, step 5 turns a route's server components into something the engine can run: a template program (ADR-0003) plus the code that produces its input. The research (D-010) settled the principle: *setup code runs in JavaScript, templates compile*. This ADR makes that concrete, using the server components of Vercel Commerce's product page (`tests/apps/commerce`). Between them, they use:

- **Component forms:** `function` and `async function`; arrows with a body and with an expression body; destructured props with defaults (`position = "bottom"`) and rest (`...props`).
- **JSX:** fragments, entities (`&copy;`), `{" "}`, template literals as children.
- **Conditionals and lists:** `cond ? <Prose/> : null`, `cond ? "." : ""`, `.map()` returning keyed JSX.
- **Spreads:** `{...props}` on a host element (`<svg>`) and on a client component (`next/image`).
- **Raw HTML:** `dangerouslySetInnerHTML={{ __html }}`.
- **Calls in templates:** `clsx(...)`, `new Intl.NumberFormat(...).format(...)`, `JSON.stringify(...)`.
- **Setup code:** module-level (`const { SITE_NAME } = process.env`) and async (`await getMenu(...)`, `await props.params`, `notFound()`).
- **Suspense:** `<Suspense fallback={<div>…</div>}>`.

## Decision

### 1. JavaScript computes every value; the template only places them

A server component compiles into two parts:

- **A data function** (JavaScript): the component's own code, unchanged, except that the returned JSX is replaced by a **data array**. The array holds the values of the template's *holes*, in template order. Setup code, the hole expressions (`clsx(…)`, `new Intl.NumberFormat(…).format(…)`, `` `${x}` ``, `a ? "." : ""`), destructuring, defaults and rest props all run as written, with exact JavaScript semantics.
- **A template:** the returned JSX's structure: static markup, plus holes where values go. It is lowered to a template program.

The engine (Rust) does what made it fast in the gate (D-026): it copies static chunks, and it escapes and serializes the values. It never evaluates JavaScript. Reimplementing JavaScript semantics in Rust would be a large source of subtle differences, for no measured gain.

Example: `Price` (components/price.tsx).

```tsx
const Price = ({ amount, className, currencyCode = "USD", currencyCodeClassName }) => (
  <p suppressHydrationWarning={true} className={className}>
    {`${new Intl.NumberFormat(undefined, { style: "currency", currency: currencyCode, currencyDisplay: "narrowSymbol" }).format(parseFloat(amount))}`}
    <span className={clsx("ml-1 inline", currencyCodeClassName)}>{`${currencyCode}`}</span>
  </p>
);
```

```
template:  <p class={$0}>{$1}<span class={$2}>{$3}</span></p>
           Flight: ["$","p",null,{"suppressHydrationWarning":true,"className":$0,"children":[$1,["$","span",null,{"className":$2,"children":$3}]]}]
data:      const Price$data = ({ amount, className, currencyCode = "USD", currencyCodeClassName }) =>
             [className, `${new Intl.NumberFormat(…).format(parseFloat(amount))}`, clsx("ml-1 inline", currencyCodeClassName), `${currencyCode}`];
```

### 2. Where JSX may appear

JSX compiles to a template in these positions:
- a component's return value (including arrow expression bodies)
- children and props of JSX elements (`Suspense`'s `fallback`, client components' `children`)
- either branch of `?:` and the right side of `&&` inside JSX (alongside `null`, `false`, `undefined` or `""`)
- the result of a `.map(callback)` inside JSX

JSX anywhere else is reported (a new code, `TSX2RS_E0005`), and the route falls back: stored in a variable, returned from a helper function, or passed to a function. These positions cover every component on the Commerce product page.

### 3. Server components are inlined per call site; client components become islands

- **Server components:** a server component rendered by another (`<Label …/>` inside `GridTileImage`) is inlined. Its template is placed at the call site, and the parent's data array holds the child's data array, produced by calling `Label$data(props)` with the props as written.
- **Client components:** these become islands. Their HTML comes from the island cache (D-012); literal props become static Flight chunks and expression props become values (as #15 classified them).
- **Slots:** `children` and other JSX-valued props of a *server* component are compile-time slots. `{children}` in the component's template renders the slot's template. Using `children` as a value (e.g. `React.Children.map`) is reported.
- **Spreads:** `{...props}` on a host element or a client component is expanded at compile time when the object's keys are known statically. They are known when the props come from a call site that wrote them out: `GridTileImage`'s rest props are `alt`, `src`, `fill` and `sizes`, and `LogoIcon`'s are `className`. A component's template therefore depends on its props' *keys* (never their values). One template is lowered per (component, key shape). A spread of an object whose keys aren't known statically is reported.

### 4. Holes are typed at runtime

TypeScript types aren't available to the compiler (oxc has no type checker), and they can lie anyway. Holes carry their kind, and the encoder writes a tag with each value:

| Hole | Value at runtime | Engine |
|---|---|---|
| Child (`{x}`) | string, number or bigint: text; `true`, `false`, `null`, `undefined`: nothing; arrays of those: in sequence; anything else (an element object, a function): **fallback** | a new `Child` op that dispatches on the tag |
| Attribute (`className={x}`) | the encoder applies React's value rules for that attribute name, known at compile time: numbers stringified, booleans dropped except on `data-*`/`aria-*`, `null`/`undefined` omitted | `Attribute`, `BooleanAttribute` |
| Condition (`a ? <A/> : null`) | JavaScript truthiness, computed in the data function | `If` |
| List (`xs.map(…)`) | an array of item data arrays | `Loop` |
| Client prop (`<Image alt={x}>`) | any serializable value; a function or a class instance means **fallback** | `Value` |
| Raw HTML (`dangerouslySetInnerHTML`) | a string | `RawHtml` |

A value that the hole can't take means the request falls back to Next.js (ADR-0002). That is detected by the encoder before the engine runs, so no partial output is ever sent.

### 5. Async setup, concurrency, Suspense

- **Async:** data functions of async components are async. Within a component, setup runs as written: its `await`s are sequential if the code is.
- **Concurrency:** data for sibling child components starts concurrently and is awaited together, as React renders sibling async components concurrently.
- **Suspense:** data inside a `<Suspense>` boundary is a promise that the shell doesn't wait for. The runtime streams the boundary when it resolves (D-020).
- **Throws:** `notFound()`, `redirect()` and errors thrown by setup code propagate. Before anything is sent, the request falls back to Next.js, which then handles them its own way.

### 6. Code generation and correctness

- **Rewriting by span:** data functions are produced by span-based rewriting of the original module source, replacing returned JSX with array literals built from the original expressions' source text. Setup code, comments and module-level code stay byte-for-byte. Types are stripped later, by the build step that bundles the generated modules (step 7).
- **One traversal:** the template program and the encoder come from one traversal of the template, as in the speed gate's `ProgramSink` / `InputSink`, so their orders can't diverge.
- **Correctness test:** each construct gets a fixture. The fixture's original components rendered by React (DOM server and Flight server, the `tests/differential` oracle) must match the compiled pipeline: data function in Node, encoder, engine.

## Options considered

- **Evaluate hole expressions in Rust** (a JS subset interpreter for `clsx`, template literals, ternaries). Rejected for now: every built-in reimplemented is a place to differ from JavaScript, and the gate shows the engine is already faster than the hand-written renderer with values computed in JS. It can be revisited per construct with measurements (Stage B).
- **Rely on TypeScript types for hole kinds.** Rejected: there's no type checker in the build, types aren't checked at runtime, and `any` is common. Runtime tags cost one byte per value.
- **Compile each component once, independent of call sites,** with generic spread handling in the engine (a "write arbitrary props as attributes" op). Rejected for milestone 1: it would need React's full attribute rules for unknown names in the engine. Per-shape templates resolve every spread on the Commerce page statically. A generic op can be added later if real apps need it.
- **Generate code from a re-printed AST** (oxc's codegen). Rejected: span-based rewriting keeps the user's code exactly as written, which keeps its semantics and makes the output easy to read and debug.

## Consequences

- **Steps 5–8:**
  - **Step 5** builds the template IR and its lowering (generalizing the gate's lowering), JSX → template IR, the per-shape inlining with slots and spreads, the data-function code generation, and the React-differential fixture harness.
  - **Step 6** generates the JavaScript encoder from the same traversal.
  - **Step 8** runs data functions inside Next.js's request context, so `cookies()`, `headers()` and `notFound()` work.
- **Engine:** a new `Child` op (with differential tests against React for every child type).
- **Diagnostics:** `TSX2RS_E0005` for JSX in unsupported positions, and codes for unresolved spreads and slot misuse.
- **Optimization:** memoizing pure constructors such as `new Intl.NumberFormat(...)` (D-019) becomes an optimization of the generated data functions, added with benchmarks.

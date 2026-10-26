# ADR-0005: Rendering server components end to end: compiled templates, JavaScript values, React for the rest

- **Status:** Accepted
- **Date:** 2026-10-25

## Context

ADR-0003 fixed *what runs* a compiled route: template programs on a prebuilt engine. Milestone 1, step 4 built the compiler's front end: the module graph and the classification of components as server, client or React built-ins. This ADR decides everything in between, for any App Router app and not only the Commerce test app: how a route's server components get from source code to an HTTP response.

The design has to meet these requirements:

- **Identical or fall back (ADR-0002).** For every request, the HTML (after resolving streams) and the decoded Flight tree are exactly Next.js's.
- **Exact JavaScript semantics.** User code runs as JavaScript, as written. tsx2rs never reimplements JavaScript.
- **Real apps, not demo apps.** The compatibility study (D-010) found that only 8% of routes compile as written, 66% if setup code runs in JavaScript, and 92% if awkward expressions are lifted out as well. A design that refuses a route whenever it meets a construct it doesn't know would stay near the low end. Anything outside the compiled fast path must still be rendered correctly, in the same response.
- **Speed where it matters.** For each request, Next.js does four things:
  1. It runs the components (user code and data fetching).
  2. React's Flight server serializes the tree.
  3. Next.js decodes that Flight payload again.
  4. React's Fizz renders the decoded tree to HTML.

  The engine replaces steps 2–4 with one pass. Step 1 stays in JavaScript. Phase 0 measured that this split is where the 12–17x comes from.
- **No Rust toolchain for users**, deterministic builds and versioned formats (ADR-0003, ARCHITECTURE.md).

One fact about Next.js shapes the whole design: **Next.js's HTML is always Fizz applied to the decoded Flight tree.** So if the engine writes the Flight tree that React would write, and renders HTML from that same tree by Fizz's rules, the output is identical by construction. This holds no matter where each part of the tree came from: a compiled template, or React itself.

## Decision

### 1. The pipeline

```
build (next build + adapter)
  bundler transform ── each server module ──► original code + generated data exports   (same bundle as Next.js)
  compiler (Rust) ──── each component ───────► templates ──► template programs
  onBuildComplete ──── each route ───────────► link segments, capture Next.js's wrapper, manifest

request (runtime, Node.js)
  route match ──► Next.js request context ──► host render (React Flight, one per request)
                                                 └─ data phase: data functions, in React's order
                                                 └─ React renders the parts that need React
              ──► encoder ──► engine: HTML + Flight, shell first, then each Suspense boundary
              └─► Next.js handler (fallback)
```

Each stage is described below. Sections 2–6 are the compiler, 7–8 the engine's contract, 9–12 the runtime, and 13–14 what falls back and how this is tested.

### 2. Build: compile inside Next.js's own bundle

- **A bundler transform.** The adapter registers it through its `modifyConfig` hook: a Turbopack rule, or a loader under webpack. It runs on modules in the server-components layer. It sends each module to the compiler (through napi). The module comes back **unchanged**, plus generated exports: the data functions (section 4) and a table that maps each component function to its compiled template.
  - Because the data functions are bundled by Next.js with the original code, they share everything with it: module instances (database clients, in-memory caches), `tsconfig` paths, `process.env` inlining, `server-only`, and the transforms Next.js applies (`"use cache"`, server actions).
  - Fallback requests run the same module, through its original exports.
  - The generated code is plain TypeScript or JavaScript. Next.js strips types and minifies it as usual.
- **Programs.** The compiler writes each template's program to `.next/tsx2rs/`, named by a content hash. That keeps builds deterministic.
- **Linking and the manifest.** `onBuildComplete` runs once the build is done. For each route, it:
  - links the programs of the route's segments (section 9)
  - captures Next.js's wrapper around them (section 9)
  - writes the manifest: routes, programs, wrapper templates, and the reason for each route that falls back. The manifest is versioned JSON (invariant 3).
- **`next dev` is unchanged.** Development runs plain Next.js. `tsx2rs check` (planned) reports which routes compile, and why the rest don't.

### 3. Three ways a part of the tree is rendered

Every node of a server-component tree is rendered in one of three ways. Code shape alone never makes a route fall back.

| Path | What | Who does the work |
|---|---|---|
| **Template** | JSX in a *template position* (below) inside a server component | The engine runs the program; the data function computes the holes |
| **Element value** | A real React element that reaches a hole at runtime. Examples: JSX stored where it's inspected, `children` used as a value, components from npm packages, `React.createElement` calls | The runtime walks the element: a host element goes to the engine's generic element op (section 7); a compiled component goes back to its template; a client reference becomes an island |
| **React-rendered** | A function component that the compiler didn't compile or that uses hooks, and any value the encoder doesn't support natively | React renders it inside the request's host render (section 10); the engine turns the resulting Flight model into HTML and Flight |

The first path is fast, and the second almost as fast. The third costs about what Next.js costs for that subtree, minus Fizz. A route's speed depends on how much of it is on the first two paths. The build reports this for each route (section 13).

**Template positions** are where JSX compiles to a template:
- a component's return value, including arrow expression bodies and every `return` in the function
- children and props of JSX elements, including `Suspense`'s `fallback` and the `children` of client components
- either branch of `?:`, the right side of `&&` / `||` / `??`, and parenthesized forms of these, when the whole expression is itself in a template position
- the callback result of `.map()` on any value, when the call is in a template position. Each item gets its own data, with its `key`.
- a `const` binding whose every use is in a template position (`const badge = <Label/>; return <div>{badge}</div>`)

JSX anywhere else (passed to a function, stored in an object, returned from a helper that isn't a component) is left as **real JSX** and becomes an element value. Its semantics are React's own, because it *is* a React element: `React.Children`, `cloneElement`, `isValidElement` and `.props` all behave as in Next.js.

### 4. Data functions: the component's own code, computing the holes

A compiled server component becomes:

- **A template:** the returned JSX's structure, meaning static markup plus *holes* where values go. It is lowered to a program.
- **A data function:** the component's own code, produced by span-based rewriting of the original source. Every returned JSX expression is replaced by a *data record*: the hole values, in template order, as the original expressions written there. Setup code, destructuring, defaults, rest props, comments and module-level code stay byte for byte. This includes `clsx(…)`, `new Intl.NumberFormat(…).format(…)`, `` `${x}` ``, `cond ? "." : ""` and `JSON.stringify(…)`.

Example, `Price` (`components/price.tsx` in the Commerce app):

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

Rules that keep its semantics identical to React's:

- **Evaluation order.** A JSX expression evaluates all of its attribute and child expressions, in source order, before any child component runs. So a hole for a child component holds a *pending call*: the component and its props object, which is built exactly as the JSX transform builds it, spreads and `key` included. The runtime calls the child after the parent's record is complete. Children run depth first, in the order React's Flight server visits them (the props object's key order, with `children` last as written). A fixture records the call order and compares it with React's.
- **Concurrency.** The synchronous part of each child runs in that order. Async children are awaited together, as React awaits sibling async components.
- **Throws.** These include `notFound()`, `redirect()` and errors thrown by setup code. They propagate the way they do under React (section 12).
- **Every component kind**, wherever it is defined:
  - function declarations and `async` functions
  - arrows with a block body or an expression body
  - `export default` forms
  - components defined at module level or inside other components (a component defined inside another is a new function on every render, so it's handled as an element value)
- **Hooks.** A component that calls a hook (`use`, `useId`, or any call to a `use[A-Z]…` name) is rendered by React (path 3). React's hook dispatcher only exists while React itself calls the component. Its children and its JSX still compile. This is conservative: a false positive costs speed, never correctness.

### 5. Holes and runtime value kinds

TypeScript types aren't available to the compiler: oxc has no type checker, and types can lie anyway. Each hole has a compile-time *kind*, and the encoder tags each value at runtime:

| Hole | Value at runtime | Engine |
|---|---|---|
| **Child** (`{x}`) | A string, number or bigint becomes text. `true`, `false`, `null` or `undefined` renders nothing. Arrays and iterables render in sequence, keyed. A compiled template instance or a React element takes the element-value path. A promise, async iterable or lazy value is React-rendered. A plain object, function or symbol is an error, as in React. | `Child`: dispatch on the tag, with text separators decided at runtime |
| **Attribute** (`className={x}`) | React's rules for that attribute name, which is known at compile time (section 7) | `Attribute`, `BooleanAttribute`, `Style`, `Url` |
| **Attribute bag** (`{...rest}` on a host element) | An object built by the data function exactly as JSX builds it. Every key gets React's rules at runtime. | `Attributes` |
| **Condition** | JavaScript truthiness, computed in the data function | `If` |
| **List** (`xs.map(…)`) | An array of item records, each with its key | `Loop` |
| **Component** (`<Label …/>`) | A pending call (section 4), resolved by the runtime | linked template, or `Call` (section 6) |
| **Client prop** (`<Image alt={x}>`) | Any value React's Flight server can serialize. Strings, numbers, booleans, `null`, `undefined`, arrays and plain objects are encoded natively. Anything else (`Date`, `Map`, `Set`, `BigInt`, typed arrays, promises, server references) is serialized by React (path 3). A function is an error, as in React. | `Value` |
| **Raw HTML** (`dangerouslySetInnerHTML`) | An object with a string `__html` | `RawHtml` |

So a value reaches one of three outcomes:
- the engine places it
- React renders or serializes it, in the same response
- it is an error that React would also raise, and it is handled as Next.js handles that error

### 6. Composition: linking, slots and recursion

- **Static linking.** When a compiled component renders another compiled component under a name known at build time, the compiler links the child's program into the parent's: one flat program per route, which is what the speed gate measured (D-026). Where a call can't be linked, it becomes a `Call` op that runs the child's program from the route's program table:
  - recursion (a menu that renders itself)
  - a component known only at runtime (an element value)
  - a program above a size limit, chosen by benchmark
- **`children` and other JSX props of server components are slots.** When the callee places `{children}` only in template positions, the caller's JSX for that slot is compiled as part of the caller (the caller's data feeds it) and linked where the callee places it. When the callee uses `children` as a value in any way (`React.Children.map(children, …)`, `children.props`, passing it to a function), the caller's JSX for that prop stays real JSX (section 3). The compiler can tell which case applies, because it knows the callee (step 4).
- **Props are runtime objects.** A child's data function receives the props object exactly as React would build it. So spreads into server components (`<Label {...props} />`) and rest props need no compile-time knowledge of keys.

### 7. The engine renders the React DOM and Flight rules completely

The engine writes HTML by Fizz's rules and Flight by the Flight server's rules. It does this for every host element, not just the ones Commerce uses. This is what makes element values and React-rendered subtrees possible, and it is tested against React (section 14):

- **Attributes:**
  - property names to attribute names (`className`, `htmlFor`, `httpEquiv`, `acceptCharset`, SVG and XML names)
  - booleans and overloaded booleans (`download`, `capture`)
  - numeric attributes
  - `data-*` and `aria-*`
  - unknown attributes
  - dropping functions and symbols
  - `src` / `href` empty strings
- **`style` objects:** hyphenation, unitless numbers, `px` suffixes, custom properties and vendor prefixes.
- **URL attributes:** React 19's blocking of `javascript:` URLs.
- **Special elements:**
  - `input` attribute order, and `value` / `defaultValue` / `checked`
  - `textarea` value as content
  - `select` value marking options as `selected`
  - `option` text, `noscript`, `script`, `style` and `template`
  - void elements and `dangerouslySetInnerHTML`
- **Hoistables (React 19):**
  - `<title>`, `<meta>` and `<link>` rendered in the body are hoisted into `<head>`
  - stylesheets with `precedence` are deduplicated and ordered
  - `async` scripts are deduplicated

  The engine collects hoistables per flush and writes them where Fizz does. The shell's `<head>` is written after its body has been rendered into a buffer, which is how Fizz orders it too.
- **Text:** escaping, `<!-- -->` separators between adjacent text nodes (decided at runtime when a hole is adjacent), and empty strings.
- **Flight values:**
  - elements with keys
  - escaped `$` strings
  - `$undefined`, `$NaN`, `$Infinity`, `$-Infinity`, `$-0`, `$n` (bigint), `$D` (Date)
  - lazy references (`$L`) and client references (`I` rows)
  - Suspense boundaries
  - error rows (`E`)
  - text rows (`T`) for long strings, as React chooses them

  Anything else comes from React's own serializer (path 3) and is copied through unchanged.
- **Flight object dedupe** stays a documented fidelity gap (D-019) until it is replicated. The output decodes to an equal tree.

New ops: `Child`, `Element` (a host element whose tag or props are known only at runtime), `Attributes`, `Style`, `Url` and `Call`. Their exact form is fixed in the IR pull request. Each comes with differential tests (section 14).

### 8. Client components (islands)

- **References and props.** Client components become islands (D-012). The client reference and its props go into Flight: literal props as static chunks, other props as `Value` holes. The HTML comes from the island cache, or from React on a miss. The cache key is the component, its props, the contexts it reads, and its tree position for `useId`.
- **Server content passed into a client component** (`children` or any other prop holding JSX) is rendered by tsx2rs like any other server content.
  - The island's HTML is cached with *holes* where that content goes. The holes are found by rendering the island with unique placeholder elements.
  - If a placeholder appears exactly once, in place and unchanged, the hole is valid.
  - If not (the client component renders its children twice, transforms them, or drops them), the island can't be split. Islands like that are rendered by React with the real content (path 3).

### 9. Route segments and Next.js's wrapper

A route is a tree of segments: root layout, layouts, `template.tsx`, the page, `loading.tsx` (a Suspense boundary), `error.tsx` and `not-found.tsx` (client error boundaries), route groups, and parallel routes (`@slot`).

- **Each segment's server components** compile as above. Their programs are linked into the route's program at the positions Next.js gives them.
- **Next.js's wrapper** is captured at build time for each route shape, with slots for the segments, route params, the query string and per-request ids (D-009, D-017, D-020, step 7). The wrapper includes:
  - the document
  - the Flight `0:` row, with its router state tree
  - the `LayoutRouter` client components
  - the reveal runtime
  - the metadata and viewport boundaries
  - the global error component

  Its details are decided in the adapter's ADR (step 7). Its contract with this ADR is that segments are slots in a captured template.
- **Metadata.** `generateMetadata`, `generateViewport` and static `metadata` run as data functions. Next.js's own metadata code turns them into elements (React-rendered, path 3). Reimplementing Next.js's merging and title templates would be a source of differences.
- **Bots.** Next.js streams metadata to browsers but blocks on it for bots (`htmlLimitedBots`). The runtime classifies the user agent with Next.js's own rule and uses the matching wrapper variant.

### 10. Request time: Next.js's context and one host render

- **Next.js's request context.** Data functions call `cookies()`, `headers()`, `draftMode()`, `connection()`, `fetch` with Next.js's caching, `unstable_cache` and `after()`. These read Next.js's async-local stores. The runtime creates those stores the way Next.js's app-page handler does, through the route module's own exports, and runs the request inside them.
- **Next.js internals are pinned to a version.** The supported Next.js and React versions are listed. With any other version, every route falls back at build time, with a stated reason. The identity tests run for each supported version.
- **One host render per request.** The data phase runs inside a single React Flight render. Its root component runs the route's data functions in React's order (section 4). This gives exact React semantics where user code depends on React:
  - `React.cache` deduplicates within the request.
  - Rendering a promise, and using an async iterable, work.
  - Errors get Next.js's digests.

  The parts that need React are returned from the root to React, which renders them in the same request. These are components that use hooks, uncompiled components found at runtime, and values the encoder doesn't support. The runtime reads the host render's Flight rows as they arrive and hands them to the encoder as element trees. When nothing needs React, the host render writes one row (`0:null`).
- **Encoding and rendering.** The encoder generated from the route's template (step 6) walks the data records in program order and writes one input buffer per flush. The engine runs the program and writes HTML and Flight. There is one native call per flush (D-020).

### 11. Suspense and streaming

As D-020, generalized:
- **The shell** renders once its own data is ready.
- **Each `<Suspense>` boundary** whose data isn't ready yet is written as pending: its fallback in HTML, a lazy reference in Flight. It is rendered when its data resolves: a Flight row, `<div hidden id="S:n">`, then `$RC`.
- **Data that is ready** when the shell renders is inlined, as React does.
- **Nested boundaries and boundaries inside islands** follow React's order.
- **The host render stays open** while any boundary is pending, so a boundary's React-rendered parts can be streamed too.

### 12. Errors and Next.js control flow

- **Before the first byte:**
  - Errors, `notFound()` and `redirect()` make the request fall back to Next.js, which handles them itself: `error.tsx`, `not-found.tsx`, a 307 response with a `Location` header.
  - Rendering these natively (compiled `not-found` and error segments, Next.js's redirect response) is planned (section 15).
- **After the first byte,** only streamed boundaries can fail. An error inside a boundary is written the way React writes it:
  - in Flight, an error row with Next.js's digest (computed by Next.js's own function, in production without the message)
  - in HTML, `$RX`, so the client renders that boundary

  This is exactly what Next.js sends in the same situation.
- **A request-time fallback runs the route again** in Next.js, including its data fetching and any side effects. React's model expects server components to be pure, but side effects in render do happen. To keep fallbacks rare:
  - Every runtime check that can trigger one happens before the data phase starts (request shape, version, island cache rules).
  - Fallbacks are counted by reason (observability), so they show up in every benchmark and in shadow mode (PHASE1-PLAN section 5).

### 13. What falls back

**Build time (the whole route uses Next.js):**
- the route isn't rendered dynamically by Next.js (static or ISR pages are served from Next.js's cache, and there's nothing to gain)
- edge-runtime routes
- an unsupported Next.js or React version
- route features not yet supported (section 15)
- parse errors or unresolved imports (E0001, E0002, which also fail `next build`)

**Request time (the request uses Next.js, before the first byte):**
- a request outside the captured shapes, such as a new navigation shape or a prefetch (D-017)
- per-visitor state that isn't supported yet (D-019)
- errors and control flow before the first byte (section 12)
- an internal error (a panic becomes an error at the napi boundary, invariant 6)

**Diagnostics get a severity.**
- An **error** makes the route fall back.
- A **note** names a part of the route that React renders (path 3) and why, such as a hook call, a component from a package, or an element value.

With this ADR, `TSX2RS_E0003` (dynamic import) and `TSX2RS_E0004` (unknown component) become notes. Neither stops a route compiling any more, because the runtime knows what an imported value or element really is. The codes keep their meaning (docs/diagnostics.md is updated when the severities are implemented). The build prints each route's status (*compiled*, with its share of React-rendered parts, or *falls back: reason*), and `tsx2rs check` shows the same.

### 14. Correctness tests

- **Differential, per construct.** Every template construct, hole kind, attribute rule, special element, hoistable, Flight value type and composition rule has a fixture. The fixture's original components are rendered by React (Fizz and the Flight server, the `tests/differential` oracle) and compared with the compiled pipeline: data function in Node.js, encoder, engine.
- **Property tests.** The engine's generic paths (`Element`, `Attributes`, `Style`, `Child`) are checked against React on generated trees and props, as in step 3.
- **Identity, per app.** Fixture apps and the Commerce app are checked through `next build` and the runtime against `next start` (ADR-0002, the `tests/identity` harness).
- **Semantics.** Fixtures check call order, concurrency, `React.cache` deduplication, `cookies()` and `headers()`, errors before and after the first byte, `notFound()` and `redirect()`.
- **Fuzzing.** Program and input decoding are fuzzed, because the input is untrusted (ARCHITECTURE.md).

### 15. Delivery

The design above is the target. It is built in this order, and each step ends green:

| Step | Builds |
|---|---|
| **5** | Template IR and lowering. Template positions, data functions and pending calls. Linking, slots and `Call`. The ops `Child`, `Element`, `Attributes`, `Style` and `Url`, with the full attribute rules. Hoistables. Per-construct differential fixtures. |
| **6** | The encoder, generated from the same traversal as the program, so their orders can't diverge. Native value encoding plus the hand-off to React. |
| **7** | The adapter: the bundler transform, writing programs, linking, wrapper capture and the manifest. |
| **8** | The runtime: Next.js's request context, the host render, the data phase, streaming, islands, errors after the first byte and fallback. |
| **After milestone 1** | In order: compiled navigation and prefetch (D-017), native `notFound()` / `redirect()` / error segments, server actions passed as props or used in `<form action>` (`$h` references and Fizz's hidden inputs), `"use cache"` components, parallel and intercepting routes, and partial prerendering. |

Until each feature in the last row lands, routes that use it fall back with a stated reason.

### 16. To verify first

These are assumptions that the design depends on. The first PR that needs each one verifies it with a test. If one turns out false, a new ADR records what changes. Until then, the effect is a stated fallback reason.

| Assumption | Verified by | If false |
|---|---|---|
| The adapter can register a bundler transform for server-layer modules under Turbopack and webpack | A fixture app built by `next build` with the transform | Generate the data modules as a separate bundle with the same resolution; module instances are no longer shared, so `React.cache` and in-memory caches would need care |
| The host render's overhead is ≤ 5% of the Commerce page's CPU time | A benchmark in step 8 | Run data functions directly, inside React's request storage only |
| `React.cache` deduplicates inside the host render's async continuations | A fixture | Hooks-free components that use `cache` take path 3 |
| Next.js's request stores can be created from the route module's exports, for each supported version | A fixture with `cookies()` and `headers()`, run for each version | Routes that read request APIs fall back |

## Options considered

- **Report and fall back on every construct the compiler doesn't model** (the first draft of this ADR: JSX outside a short list of positions, unknown spreads, `children` used as a value, components from packages). Rejected. It would make the product a demo for code shaped like Commerce. Element values and React-rendered parts keep those routes compiled and identical, at a cost limited to the parts that need it.
- **Evaluate hole expressions in Rust** (a JavaScript subset for `clsx`, template literals and ternaries). Rejected. Every reimplemented built-in is a place to differ from JavaScript, and the gate shows the engine is already faster than the hand-written renderer with values computed in JavaScript. It can be revisited per construct, with measurements (Stage B).
- **Rely on TypeScript types for hole kinds.** Rejected. There's no type checker in the build, types aren't checked at runtime, and `any` is common. A runtime tag costs one byte per value.
- **Inline server components per call site, with one template per prop-key shape** (the first draft's answer to spreads). Rejected. Recursion, components known only at runtime and spreads of unknown keys can't be handled that way, and the number of templates grows with the shapes. Linking with `Call`, plus an engine that knows React's full attribute rules, handles all of them.
- **Run data functions outside React**, calling them directly. Rejected as the default: `React.cache`, rendering promises, hooks and Next.js's error digests depend on React's request scope. Kept as the fallback plan in section 16 if the host render costs too much.
- **Generate data functions as a separate bundle** (esbuild with the `react-server` condition). Rejected as the default: module instances and Next.js's transforms would differ from what Next.js runs. Kept as the fallback plan in section 16.
- **Generate code from a re-printed AST** (oxc's codegen). Rejected. Span-based rewriting keeps the user's code exactly as written, which keeps its semantics and makes the output easy to read and debug.

## Consequences

- The engine grows from Commerce's needs to React DOM's full server rules. That is a large, well-bounded piece, tested differentially against React.
- The runtime depends on Next.js internals (request stores, metadata, error digests). They are pinned per supported version and covered by the identity tests for each version.
- A route's speed depends on its share of React-rendered parts. The build reports that share for each route, and `tsx2rs check` explains it.
- `TSX2RS_E0003` and `TSX2RS_E0004` become notes, and diagnostics get severities.
- ARCHITECTURE.md's compiler phases and request data flow are updated to match this ADR.
- Memoizing pure constructors such as `new Intl.NumberFormat(...)` (D-019) becomes an optimization of the generated data functions, added with benchmarks.

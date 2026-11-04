# ADR-0014: Slots placed by the callee, and `Call` for recursion

- **Status:** Accepted
- **Date:** 2026-11-04

## Context

ADR-0013 linked child components by inlining. It left four things for a second pull request
(C5b): slots, `Call` for recursion, template nodes for components known only at runtime, and
async callees. ADR-0005 (section 6) says a slot is compiled with its caller only when the callee
*only places* the prop.

What React does, probed (react 19.3.1, production):
- A callee placing `{children}` and `{title}` writes the caller's elements in place. JSX children
  are one array: `[["$","b","k",…],"y"]` in Flight.
- A callee that *returns* `children` gives that element its key path (`"w,k"`, ADR-0008).
- An async child component is written as its own Flight row (`"$L1"`, then `1:[…]`), even when
  it resolves immediately.

## Decision

- **A prop is placed** when the callee destructures it from its props (`{ children }`, no
  default, not computed) and *every* reference to its binding is a `{prop}` hole in the callee's
  own templates. References are counted with `oxc_semantic`. Any other use doesn't count:
  - in setup code or another expression
  - inside a closure
  - passed to another component
  - inside an element value
  - at a template's root, where React gives it the key path
- **A slot** is a caller's JSX for a placed prop: a JSX prop, or JSX children (one child, or an
  array of them). It's lowered in the caller's context at each hole where the callee places it
  (`Slot::Placed`).
  - Its data is the caller's: the pending call's config holds the slot's record, in JSX order,
    with children last.
  - The callee's own data function is unchanged: it places that value, the slot's record.
  - A component element whose JSX prop or children the callee doesn't only place stays a
    `Child` hole.
- **Recursion uses `Op::Call`.** A callee already being lowered becomes `Call` to a template of
  its own, made once per component and lowered with none of its callers around it.
  - Its record is in `Lowered::templates`.
  - A recursive call that passes slots stays a `Child` hole, since a shared template can't hold
    one caller's slots.
  - An inlining that fails is undone together with the templates made inside it.
- **Moved out of the compiler half,** with reasons:
  - **Async callees** need Flight rows of their own (`$L`), which come with streaming and the
    runtime (step 8). They stay `Child` holes until then.
  - **Template nodes** for components known only at runtime are written by the encoder (step 6).
    It needs a table from component to template, which the route's program table provides then.
  - **A size limit** for inlining is chosen by benchmark (ADR-0008), once whole routes compile.

## Options considered

- **Treat every `children` prop as a slot.** It would be wrong whenever a callee inspects or
  passes on `children`. Rejected (ADR-0005).
- **Pass slots as real JSX and render them in the runtime.** It's correct but slow, and it's
  what an unplaced prop already does. Kept only as the fallback.
- **One template per (component, slots) pair for recursion.** It's rare, and its size is
  unbounded. Rejected for now.

## Consequences

- `Slot::Call` now holds its body (`Inline` or `Template`) and its slot records, and
  `Slot::Placed` is new. Together with `Lowered::templates`, these are the encoder's input
  (step 6).
- `Component::placed` lists each component's placed props and their holes.

# ADR-0008: Templates call templates: one program per route, with a template table

- **Status:** Accepted
- **Date:** 2026-10-26

## Context

ADR-0005 (section 6) links a compiled child component into its caller's program where it can,
giving one flat program per route. Where it can't, a `Call` op "runs the child's program from the
route's program table":

- recursion (a menu that renders its submenus)
- a component known only at runtime: an element value whose type is a compiled component
- a program above a size limit, chosen by benchmark

It leaves the exact form to the pull request that adds the op. Until now a `Program` was one
list of ops.

## Decision

- **A program is a chunk table plus a template table.** Each template is a list of ops. Template
  0 is the route's entry. All templates share the chunk table, so a route is still one file and
  one validation.
- **`Op::Call { template }`** (opcode 14) runs another template in place. It reads that template's
  values from the same input, at that point, in program order. It writes nothing itself: a server
  component has no output of its own in HTML or Flight. Text state carries across the call, so
  React's `<!-- -->` separators come out the same as if the template were inlined.
- **A template node** (`tag::TEMPLATE`, 10) in `Op::Child`'s input: a `u32` template index, then
  that template's input. This is how a compiled component known only at runtime gets rendered:
  the encoder writes which template, then its values. An index out of range is
  `RenderError::BadTemplate`.
- **Each run of a template has its own loop stack,** so a template can call itself. **Calls,
  element nesting and values share one depth limit** (`MAX_VALUE_DEPTH`), so recursion can't
  overflow the stack; going past it is `TooDeep`.
- **Validation per template,** with errors as `FormatError::InTemplate`. Every `Call` must name a
  template that exists. A loop body may read its input through a call. Which templates read input
  is computed as a fixed point, so a loop whose body only calls a template that reads nothing is
  still rejected.
- **Where the engine can't run a template:** wherever React turns a component into a string (a
  `<textarea>`'s children, an `<option>`'s text with a selected value), it returns
  `UnsupportedElement` and the request falls back. Only running a template tells where its input
  ends, and React itself writes `[object Object]` there.

### Key paths are computed by the runtime

React's Flight server gives a server component's direct output a *key path*
(`renderFunctionComponent` and `renderElement` in react-server 19.3.1):

- The keys of the component elements above it are joined with `,`, and the element the component
  returns gets that path in front of its own key. So `items.map(i => <Item key={i.id}/>)` writes
  each `Item`'s root `<li>` with the key `i.id`.
- When the component has no key, the element it returns is wrapped in an array (`[element]`) if
  that element has a key of its own (React's `implicitSlot`).
- An array returned under a key path becomes a keyed fragment.
- None of this reaches further down: an element's children are serialized after the component
  returns.

The runtime already resolves every pending call (ADR-0005, section 4), so it knows each component
element's key and how they nest. **It computes the key path, and the encoder writes the result as
input:** a template reads its root element's Flight key (with `Value`), and whether to wrap it
(with `If`). The engine needs nothing for it. The compiler emits these holes for templates whose
root is an element, inlined or called alike. Keyed fragments come with the encoder. The
differential test covers the wrap, with React rendering each compiled component as a real
function component.

## Options considered

- **A table of separate programs, each with its own chunks.** It duplicates shared chunks, and
  validation would have to look across programs anyway for calls. Rejected.
- **Inline everything at build time.** Recursion and runtime components can't be inlined.
  Rejected as the only mechanism; it stays the default where possible.
- **Calls with explicit arguments** (a frame of values). The input is already in program order,
  so the callee's values are simply next. Rejected as unnecessary.
- **Key paths in the engine** (a call reads the component element's key, and the engine rewrites
  the callee's root key and wraps it). It would need the root element's Flight key in a known
  place, which fails for element values and for roots that are holes, and it duplicates what the
  runtime knows anyway. Rejected.

## Consequences

- The binary format gets a template count, and an op count per template.
- `Program::new` keeps building a one-template program. `Program::with_templates` builds the rest.
- The compiler's linking step decides, per call, between inlining (the default), `Call`, and a
  template node.
- Client components inside element values (islands in values) are a separate step.

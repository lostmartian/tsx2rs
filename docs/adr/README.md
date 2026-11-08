# Architecture Decision Records

Decisions that shape this codebase: its boundaries, formats, invariants and tooling. Each record is short, permanent and numbered. To change a decision, write a new ADR that supersedes the old one, and mark the old one `Superseded by ADR-NNNN`.

Format: [MADR](https://adr.github.io/madr/)-style. Context → Decision → Consequences, plus the options considered.

| ADR | Title | Status |
|---|---|---|
| [0001](0001-record-architecture-decisions.md) | Record architecture decisions | Accepted |
| [0002](0002-identical-output-to-nextjs.md) | Output identical to Next.js, or fall back | Accepted |
| [0003](0003-template-programs-on-a-prebuilt-engine.md) | Template programs on a prebuilt Rust engine | Accepted |
| [0004](0004-engineering-standards.md) | Engineering standards: tooling, testing and git workflow | Accepted |
| [0005](0005-compiling-server-components.md) | Rendering server components end to end: compiled templates, JavaScript values, React for the rest | Accepted |
| [0006](0006-instruction-counts-for-pull-requests.md) | Judge pull requests' performance by instruction counts, measured in CI | Accepted |
| [0007](0007-element-values.md) | Element values are a node kind that `Child` reads | Accepted |
| [0008](0008-template-calls.md) | Templates call templates: one program per route, with a template table | Accepted |
| [0009](0009-hoistables-in-place.md) | Hoistable elements rendered in place, and the document's elements inside the body | Accepted |
| [0010](0010-hoisting-into-head.md) | Hoisting into `<head>`: the engine collects React's queues, the runtime places them | Accepted |
| [0011](0011-flight-resource-hints.md) | Flight resource hints, and hinted elements always as element values | Accepted |
| [0012](0012-compiling-templates.md) | Compiling templates: template trees, holes, folding through the engine | Accepted |
| [0013](0013-linking-pending-calls.md) | Linking by inlining, with pending calls as `jsx` arguments | Accepted |
| [0014](0014-slots-and-calls.md) | Slots placed by the callee, and `Call` for recursion | Accepted |
| [0015](0015-component-fixtures.md) | Component fixtures run the whole compiler pipeline, and what they changed in records | Accepted |
| [0016](0016-generated-encoder.md) | The generated encoder: straight-line code per template, on a small runtime library | Accepted |
| [0017](0017-adapter-build.md) | The adapter's build: compile before bundling, data functions in Next.js's own bundle | Accepted |
| [0018](0018-capturing-the-wrapper.md) | Next.js's wrapper is captured from real responses, then confirmed | Accepted |
| [0019](0019-runtime-server.md) | The runtime server hands everything it doesn't serve to Next.js's own request handler | Accepted |
| [0020](0020-islands.md) | Client component islands are learned from Next.js's responses | Accepted |
| [0021](0021-supported-nextjs-version.md) | One supported Next.js version, moved deliberately | Accepted |
| [0022](0022-metadata-per-request.md) | A page's metadata is learned per request when the params don't explain it | Accepted |
| [0023](0023-per-request-wrappers.md) | A wrapper that depends on the request is kept per URL | Accepted |
| [0024](0024-suspense-boundaries.md) | Suspense boundaries are compiled, and their form is learned from Next.js | Accepted |
| [0025](0025-flight-rows.md) | The engine splits Flight into rows as React does, and async components are compiled | Accepted |
| [0026](0026-object-identity.md) | Objects React has written before are written as references | Accepted |
| [0027](0027-re-checking.md) | Served routes are re-checked against Next.js | Accepted |
| [0028](0028-tsx2rs-start.md) | `tsx2rs start` runs the app, with a worker per core when asked | Accepted |

Background research (benchmarks, spikes, the full decision history before this repository existed) lives in the separate `tsx2rs-lab` repository.

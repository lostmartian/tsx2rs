# @tsx2rs/runtime

What a compiled route runs at request time. Today that's encoding, capturing Next.js's wrapper, and the server; serving compiled routes through it is next (milestone 1, step 8).

- **`@tsx2rs/runtime/encode`** ([ADR-0016](../../docs/adr/0016-generated-encoder.md)): the engine's input writer (`Input`), and the encoders the compiler's generated encoders call:
  - one per hole kind: `node`, `primitive`, `style`, `attrs`, `value`
  - `call`, which resolves a pending call as React's `jsx` does and runs the callee's data function
  - `rootKey`, which puts React's key path in front of a root element's key

  A value the engine can't take natively (a promise, a component element, a function, a `Date`) throws `NotNative`, where the route will hand it to React.

- **`@tsx2rs/runtime/capture`** ([ADR-0018](../../docs/adr/0018-capturing-the-wrapper.md)): Next.js's wrapper around a compiled page, captured from real responses.
  - `learn` takes one response and the fragments the runtime rendered for it.
  - `confirm` takes a response with different params: every byte must match, or the route is refused.
  - `fill` writes the document for a request.

  It is tested byte for byte on real Next.js 16.4.5 documents (`test/next/`).

- **`@tsx2rs/runtime/server`** ([ADR-0019](../../docs/adr/0019-runtime-server.md)): `start({ dir, port })` serves an app built with `@tsx2rs/adapter`. A request tsx2rs doesn't serve goes to Next.js's own request handler, so it is `next start`'s answer, byte for byte. A compiled route learns Next.js's wrapper from Next.js's own responses, confirms it with a second response for other params, then serves its pages itself, byte for byte (`x-tsx2rs: compiled`); a route it can't render natively yet is refused, with the reason, and stays Next.js's. `pnpm test:runtime` checks that against `next start` on a real build.

Tested with `pnpm test` (unit tests, byte for byte) and end to end by the component fixtures in [`tests/differential`](../../tests/differential/README.md), which run every generated encoder against a reference and the engine's output against React.

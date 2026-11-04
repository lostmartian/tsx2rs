# @tsx2rs/adapter

The tsx2rs Next.js [Deployment Adapter](https://nextjs.org/docs/app/api-reference/config/next-config-js/adapterPath) ([ADR-0017](../../docs/adr/0017-adapter-build.md)). Add one line to `next.config`:

```ts
const config = { adapterPath: import.meta.resolve("@tsx2rs/adapter") };
```

`next build` then does three things:
- **Compiles the app's pages,** once, before bundling. Each route **compiles** or **falls back** to Next.js, with the reason printed: `[tsx2rs] app/p/[n]/page.tsx: compiled`.
- **Adds each compiled server module's data functions** to Next.js's own server bundle, through a loader for Turbopack and webpack. Client bundles are exactly what Next.js builds without the adapter.
- **Writes `<distDir>/tsx2rs/manifest.json`,** a versioned file with each route's pathnames, Next.js entrypoint, program and encoder, and the program and encoder files next to it.

The runtime that serves compiled routes from the manifest comes with milestone 1's step 8; until then, the output is the input for it.

Tests:
- `pnpm test` runs the unit tests.
- `pnpm test:adapter` builds `test-e2e/app` with real `next build`s, with Turbopack and webpack, and checks the manifest, the data functions in the server bundle (loaded the way the runtime will load them), and that client bundles are unchanged.

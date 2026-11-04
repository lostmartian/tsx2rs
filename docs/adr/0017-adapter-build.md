# ADR-0017: The adapter's build: compile before bundling, data functions in Next.js's own bundle

- **Status:** Accepted
- **Date:** 2026-11-05

## Context

Milestone 1's step 7 makes `next build` produce everything a compiled route needs, with no manual
steps. ADR-0005 (section 9) and ARCHITECTURE.md decided the shape:
- a bundler transform adds each server module's data functions to Next.js's own bundle, next to
  the unchanged original code;
- `onBuildComplete` writes the manifest.

Three things were open:
1. whether a Deployment Adapter can transform only the **server** copies of modules, under
   Turbopack (Next.js 16's default) as well as webpack;
2. how a runtime outside Next.js finds the data functions inside that bundle;
3. where the compiler runs.

A spike in the lab repository (`spikes/adapter-transform`) answered 1 and 2 on Next.js 16.4.3,
with both bundlers.
- **Transform:** a loader rule added in `modifyConfig`
  (`turbopack.rules["*.tsx"] = { loaders, condition: { not: "browser" } }`, and for webpack a rule
  on the server compiler) changes `.next/server` only, never `.next/static`.
- **Reach:** code it appends runs when the route's modules evaluate. Requiring a route's
  entrypoint doesn't evaluate them, since Next.js loads segments lazily. Calling the getters in
  its `routeModule.userland.loaderTree` does.

## Decision

- **The compiler runs once, before bundling.** `modifyConfig`, in the production-build phase,
  calls `compileApp(root, pages)` (napi, `compiler::build`) for the app's `page.*` files and keeps
  the result for the loader and `onBuildComplete`. Each page **compiles** or **falls back** with
  its reasons. A reason is a diagnostic, a client page, a page without a default-exported
  component, or a lowering error.
- **The loader appends, never rewrites.** For each server module with components, it appends the
  module's data functions (`data::functions`), then registers them under the module's project
  path:
  `(globalThis[Symbol.for("tsx2rs.data")] ??= new Map()).set(path, { __tsx2rs_data_X, … })`.
  The data functions share the module's own scope and imports, which also gives the same
  `React.cache` and Next.js internals as the page. A module the compiler didn't compile is passed
  through unchanged.
- **The runtime (step 8) finds them** by loading the route's entrypoint, calling its loader
  tree's getters, and reading the registry. There it also finds the route's program, encoder and
  callees, from the manifest.
- **The manifest** is written by `onBuildComplete` into `distDir`. It is versioned JSON plus one
  file per program:
  - for each page route: its pathname, Next.js's entrypoint (`outputs.appPages`), and either the
    program, encoder, entry and callees, or the fallback reasons;
  - the build id and the Next.js version.

  The build prints *compiled* or *falls back: reason* for every route.
- **Three pull requests:**
  - 7.1 `compiler::build` and `compileApp`
  - 7.2 `@tsx2rs/adapter`: `modifyConfig`, the loader, `onBuildComplete` and the manifest,
    tested by building fixture apps with `next build`
  - 7.3 build-time capture of Next.js's wrapper around the page: the document, the layouts'
    output and the Flight wrapper

## Options considered

- **Bundle the data functions ourselves** (esbuild, separately from Next.js). The module
  instances wouldn't be Next.js's: `React.cache`, `next/headers` and module state would differ.
  Rejected (ADR-0005).
- **Export the data functions** and reach them through module exports. Only segment modules
  (page, layout) are reachable from the entrypoint, not the components they import. Rejected for
  the registry.
- **Compile in the loader, module by module.** Linking needs the whole route's graph, so each
  loader call would rebuild it. Rejected for one compile before bundling.

## Consequences

- The registry is the one piece of process-global state in the JavaScript runtime. It is
  written only by modules as they evaluate, and keyed by project path, so builds and routes
  can't collide.
- A route compiles or falls back as a whole at build time. Parts the engine can't render
  natively are still handed to React at request time (ADR-0016).
- On Commerce today, 4 of 6 page routes compile. The search pages fall back, because
  `product-grid-items.tsx` uses a `<A.B>` member tag from a non-namespace import.

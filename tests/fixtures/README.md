# Fixture apps

One minimal Next.js app per construct, named `<area>-<feature>` (e.g. `jsx-basics`, `suspense-streamed`). The [identity harness](../identity/README.md) builds each one and compares its output.

Every fixture has:

- `README.md`: what it covers
- `expect.json`: what to check
  - `urls`: the paths to request, each starting with `/`
  - `streams` (optional, default `false`): whether Next.js streams at least one `<Suspense>` boundary for them
- `app/`: the Next.js app. Pages are dynamic (`export const dynamic = "force-dynamic"`), so every request renders.
- `package.json` taking `next`, `react` and `react-dom` from the workspace catalog (`pnpm-workspace.yaml`), so every fixture tests the same versions
- `tsconfig.json` as Next.js writes it, with `"strict": true`

**Add a fixture for every construct tsx2rs supports and for every reason it falls back** (docs/engineering/testing.md).

# tsx2rs

**Make Next.js cheap to self-host.** tsx2rs is a Next.js [Deployment Adapter](https://nextjs.org/docs/app/api-reference/config/next-config-js/adapterPath) that compiles your React server component templates into programs run by a native Rust engine. Client components are served from an HTML cache, and anything it can't compile falls back to Next.js unchanged. No code changes and no Rust toolchain needed: add one line to `next.config`.

> **Status: early development (Phase 1, milestone 1).** Not usable yet.

## Why

Measured on a real app (Vercel Commerce, hand-compiled prototype, Google Cloud `c3-standard-8`), with output identical to Next.js:

- **12–17x the throughput of `next start`** with every core in use
- **~20x lower median latency** under load
- Streaming `<Suspense>`: first byte in **31–34 ms** instead of 268–883 ms

## How it works

1. **Build:** the adapter compiles each eligible route's server component templates into a *template program* (static HTML/Flight chunks + a few ops) and a generated data encoder.
2. **Request:** your data-fetching code still runs in Node.js. Its results go to the Rust engine in one call, which writes the HTML and the RSC (Flight) payload.
3. **Client components** render with React and are cached. Unsupported code makes the route fall back to Next.js.

## Layout

| Path | What |
|---|---|
| `crates/` | Rust: template IR, compiler (oxc), engine, napi bindings |
| `packages/` | npm: `@tsx2rs/adapter`, `@tsx2rs/runtime`, `@tsx2rs/native` |
| `tests/` | identity tests against Next.js, fixtures, the Commerce app |
| `bench/` | benchmarks |

## License

[Apache-2.0](LICENSE)

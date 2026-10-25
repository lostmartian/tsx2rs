# Identity harness

Decides whether a tsx2rs response is identical to what Next.js produces for the same request. This is the project's main correctness test ([ADR-0002](../../docs/adr/0002-identical-output-to-nextjs.md)).

Two documents are identical when:

1. **HTML:** after applying React's streaming instructions (`$RC`, `$RS`) the way the browser does, and removing the Flight scripts, they are byte-identical. Next.js decides per request which `<Suspense>` boundaries stream, so the raw bytes are never compared.
2. **Flight:** their RSC payloads decode to deep-equal trees, with every reference resolved and hints included. React's row splitting doesn't matter, but every value does.

Anything the harness doesn't model exactly throws an `IdentityError` with a stable code. It never approximates, because an approximation could hide a difference.

| Module | Does |
|---|---|
| `src/streaming.ts` | Applies `$RC`/`$RS` to the HTML (React DOM 19.3.1 semantics) |
| `src/flight.ts` | Extracts the payload from `self.__next_f` pushes, parses rows, decodes values (React 19.3.1 Flight client semantics) |
| `src/compare.ts` | Compares two documents and reports the first difference (offset/line/column, or tree path) |
| `src/fixtures.ts` | Loads `tests/fixtures/*/expect.json` |
| `src/next.ts` | Builds and serves an app with its own `next` binary |

## Running

```bash
pnpm test              # unit and property tests (fast)
pnpm test:identity     # builds every fixture with Next.js and checks it against itself
```

Today the fixture tests compare Next.js with Next.js: identical across servers and requests, and one-byte changes to real output are caught. Once the tsx2rs runtime exists (milestone 1, step 8), the same tests compare tsx2rs with Next.js.

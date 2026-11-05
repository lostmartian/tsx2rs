# Next.js documents

Real responses from Next.js 16.4.3 (`next start`, Turbopack build) for the adapter's test page `app/q/[n]/page.tsx` (`packages/adapter/test-e2e/app`), used by `capture.test.ts` (ADR-0018):

| File | Request |
|---|---|
| `q-3.html` | `/q/3` |
| `q-7.html` | `/q/7` |
| `q-long.html` | `/q/1234567890123456789012345678901234567890` |
| `q-escaped.html` | `/q/a%20b%26c` (Next.js passes the page `n = "a%20b%26c"`, undecoded) |
| `q-3-query.html` | `/q/3?x=1&y=%3Cb%3E` |

To refresh them, build the app, then run `npx next start -p 3471` in it and `curl -s localhost:3471/<request> > <file>`. Chunk names change with every build, so refresh all of them together.

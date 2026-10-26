## What and why

<!-- What does this change, and why is it needed? Link the issue/ADR. -->

## How it was tested

<!-- Which tests were added or changed? Identity fixtures? Browser check? -->

## Correctness and fallback

- [ ] Output is identical to Next.js for every affected case (identity test or fixture)
- [ ] Unsupported input falls back with a diagnostic code and reason

## Performance

<!-- Hot-path change? Summarize the "Benchmarks" job's instruction counts (CI) and justify any regression above 5%. Otherwise: "not on a hot path". -->

## Checklist

- [ ] Tests at the right level; bug fixes include a regression test
- [ ] No new `unwrap`/`expect`/`panic` in library code; any `unsafe` has a `// SAFETY:` comment
- [ ] Docs updated (public items, ARCHITECTURE.md, ADR if boundaries/formats changed)
- [ ] New dependencies justified (license, maintenance)

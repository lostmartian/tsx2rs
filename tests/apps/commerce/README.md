# Test app: Vercel Commerce

A copy of [vercel/commerce](https://github.com/vercel/commerce) (commit `f7521d5`, MIT, see [`license.md`](license.md)), used as the end-to-end test app for milestone 1: the compiler must reproduce its product pages exactly (`/product/[handle]` and `/product-streaming/[handle]`).

**Changed from upstream:**
- **Next.js 16.4.3 / React 19.3.1:** `next.config.ts` drops the 15-canary experiments (`ppr`, `useCache`, `inlineCss`), which Next.js 16 renamed or removed.
- **`lib/shopify/index.ts` and `lib/shopify/fixtures.ts`:** local test data instead of the Shopify Storefront API. Same exports and signatures, deterministic, no network, with an in-memory cart keyed by the `cartId` cookie. The data and descriptions are unchanged from the tsx2rs research measurements, so results stay comparable.
- **Simulated latency per data call:** `LATENCY_<PRODUCT|RECOMMENDATIONS|MENU|CART>_MS`, default 0.
- **`app/product-streaming/[handle]/page.tsx`:** the product page with `<RelatedProducts>` wrapped in `<Suspense>`, so its boundary streams when recommendations are slow.
- **`public/images/`:** generated test images. `.env.production` holds stub values.

**Not changed:** every other page, layout and component. The upstream README is kept as [`README.upstream.md`](README.upstream.md). `fonts/Inter-Bold.ttf` comes from upstream (used by the Open Graph image routes; Inter is under the SIL Open Font License).

This folder is third-party code: the repository's ESLint and Prettier skip it. It isn't a pnpm workspace package yet; the steps that build and run it add that.

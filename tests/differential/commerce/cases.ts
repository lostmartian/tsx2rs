/**
 * Props for Commerce's own server components, built from every product in its test data
 * (`tests/apps/commerce/lib/shopify/fixtures.ts`), the way its pages pass them: step 6's round
 * trip of real data through the encoder (ADR-0016). `tests/commerce.rs` checks each component
 * against React.
 */
import type { Product } from "../../apps/commerce/lib/shopify/types.ts";

// The vendored app isn't an ES module package, so its source stays out of this package's
// type-check: loaded through a URL, typed by its own `Product`.
const fixtures = new URL("../../apps/commerce/lib/shopify/fixtures.ts", import.meta.url);
const { products } = (await import(fixtures.href)) as { products: Product[] };

const prices = products.map((p) => p.priceRange.maxVariantPrice);

/** `components/price.tsx`, as `ProductDescription` and `Label` call it. */
export const price = [
  ...prices.map((m) => ({ amount: m.amount, currencyCode: m.currencyCode })),
  ...prices.slice(0, 3).map((m) => ({
    amount: m.amount,
    currencyCode: "EUR",
    className: "flex-none rounded-full bg-blue-600 p-2 text-white",
    currencyCodeClassName: "hidden @[275px]/label:inline",
  })),
];

/** `components/label.tsx`, as `GridTileImage` calls it. */
export const label = products.map((p, i) => ({
  title: p.title,
  amount: p.priceRange.maxVariantPrice.amount,
  currencyCode: p.priceRange.maxVariantPrice.currencyCode,
  position: i % 2 === 0 ? "bottom" : "center",
}));

/** `components/prose.tsx`, as `ProductDescription` calls it. */
export const prose = products.map((p) => ({
  className: "mb-6 text-sm leading-tight dark:text-white/[60%]",
  html: p.descriptionHtml,
}));

/** `components/grid/tile.tsx` without an image: its label links `Label` and `Price`. */
export const tile = products.map((p, i) => ({
  active: i === 0,
  label: {
    title: p.title,
    amount: p.priceRange.maxVariantPrice.amount,
    currencyCode: p.priceRange.maxVariantPrice.currencyCode,
  },
}));

/** `components/grid/tile.tsx` with an image: `next/image` is a client component. */
export const tileWithImage = products.slice(0, 2).map((p) => ({
  alt: p.title,
  src: p.featuredImage.url,
  fill: true,
  sizes: "(min-width: 768px) 33vw, 100vw",
}));

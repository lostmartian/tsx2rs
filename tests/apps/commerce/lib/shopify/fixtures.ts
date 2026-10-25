// tsx2rs test app: local test data replacing the Shopify Storefront API (deterministic, no network).
import type { Cart, Collection, Image, Menu, Page, Product, ProductVariant } from "./types";

const img = (handle: string, i: number, alt: string): Image => ({
  url: `/images/${handle}-${i}.png`,
  altText: alt,
  width: 800,
  height: 800,
});

const money = (amount: string) => ({ amount, currencyCode: "USD" });

function simpleProduct(n: number, handle: string, title: string, price: string, tags: string[] = []): Product {
  const image = img(handle, 1, title);
  const variant: ProductVariant = {
    id: `gid://shopify/ProductVariant/${1000 + n}`,
    title: "Default Title",
    availableForSale: true,
    selectedOptions: [{ name: "Title", value: "Default Title" }],
    price: money(price),
  };
  return {
    id: `gid://shopify/Product/${n}`,
    handle,
    availableForSale: true,
    title,
    description: `${title}: a test product for the tsx2rs real-app spike.`,
    descriptionHtml: `<p>${title}: a test product for the <strong>tsx2rs</strong> real-app spike.</p>`,
    options: [{ id: `gid://shopify/ProductOption/${n}`, name: "Title", values: ["Default Title"] }],
    priceRange: { maxVariantPrice: money(price), minVariantPrice: money(price) },
    variants: [variant],
    featuredImage: image,
    images: [image],
    seo: { title, description: `Buy ${title}` },
    tags,
    updatedAt: "2026-10-01T00:00:00Z",
  };
}

// The main product: several images, two options, a variant matrix with some sold-out combinations.
const colors = ["Black", "White", "Blue"];
const sizes = ["S", "M", "L", "XL"];
const shirtHandle = "acme-geometric-circles-t-shirt";
const shirtImages = [1, 2, 3, 4, 5].map((i) => img(shirtHandle, i, `Acme Geometric Circles T-Shirt, view ${i}`));
const shirtVariants: ProductVariant[] = colors.flatMap((color, ci) =>
  sizes.map((size, si) => ({
    id: `gid://shopify/ProductVariant/${100 + ci * 10 + si}`,
    title: `${color} / ${size}`,
    availableForSale: !(color === "Blue" && size === "XL") && !(color === "White" && size === "S"),
    selectedOptions: [
      { name: "Color", value: color },
      { name: "Size", value: size },
    ],
    price: money(size === "XL" ? "25.00" : "20.00"),
  })),
);
const shirt: Product = {
  id: "gid://shopify/Product/1",
  handle: shirtHandle,
  availableForSale: true,
  title: "Acme Geometric Circles T-Shirt",
  description: "60% combed ringspun cotton/40% polyester jersey tee. Soft, durable & lightweight.",
  descriptionHtml:
    "<p>60% combed ringspun cotton/40% polyester jersey tee.</p><ul><li>Soft &amp; durable</li><li>Lightweight</li><li>Machine washable</li></ul>",
  options: [
    { id: "gid://shopify/ProductOption/1", name: "Color", values: colors },
    { id: "gid://shopify/ProductOption/2", name: "Size", values: sizes },
  ],
  priceRange: { maxVariantPrice: money("25.00"), minVariantPrice: money("20.00") },
  variants: shirtVariants,
  featuredImage: shirtImages[0]!,
  images: shirtImages,
  seo: { title: "Acme Geometric Circles T-Shirt", description: "Soft, durable, lightweight tee." },
  tags: ["shirts", "featured"],
  updatedAt: "2026-10-01T00:00:00Z",
};

export const products: Product[] = [
  shirt,
  simpleProduct(2, "acme-drawstring-bag", "Acme Drawstring Bag", "12.00", ["bags"]),
  simpleProduct(3, "acme-cup", "Acme Cup", "15.00", ["kitchen"]),
  simpleProduct(4, "acme-mug", "Acme Mug", "15.00", ["kitchen"]),
  simpleProduct(5, "acme-hoodie", "Acme Hoodie", "50.00", ["shirts", "featured"]),
  simpleProduct(6, "acme-baby-onesie", "Acme Baby Onesie", "10.00", ["shirts"]),
  simpleProduct(7, "acme-baby-cap", "Acme Baby Cap", "10.00"),
  simpleProduct(8, "acme-sticker", "Acme Sticker", "4.00", ["stickers"]),
  simpleProduct(9, "acme-keyboard", "Acme Keyboard", "150.00"),
  simpleProduct(10, "acme-slip-on-shoes", "Acme Slip-On Shoes", "45.00", ["featured"]),
  simpleProduct(11, "acme-rainbow-sticker", "Acme Rainbow Sticker", "4.00", ["stickers"]),
  simpleProduct(12, "acme-prism-t-shirt", "Acme Prism T-Shirt", "25.00", ["shirts"]),
  simpleProduct(13, "acme-dog-sweater", "Acme Dog Sweater", "20.00"),
];

export const menus: Record<string, Menu[]> = {
  "next-js-frontend-header-menu": [
    { title: "All", path: "/search" },
    { title: "Shirts", path: "/search/shirts" },
    { title: "Stickers", path: "/search/stickers" },
  ],
  "next-js-frontend-footer-menu": [
    { title: "Home", path: "/" },
    { title: "About", path: "/about" },
    { title: "Terms & Conditions", path: "/terms-conditions" },
    { title: "Shipping & Return Policy", path: "/shipping-return-policy" },
    { title: "Privacy Policy", path: "/privacy-policy" },
    { title: "FAQ", path: "/frequently-asked-questions" },
  ],
};

export const collections: Collection[] = ["shirts", "stickers", "kitchen", "bags"].map((handle) => ({
  handle,
  title: handle[0]!.toUpperCase() + handle.slice(1),
  description: `All ${handle}`,
  seo: { title: handle, description: `All ${handle}` },
  updatedAt: "2026-10-01T00:00:00Z",
  path: `/search/${handle}`,
}));

export const pages: Page[] = ["about", "terms-conditions", "shipping-return-policy", "privacy-policy", "frequently-asked-questions"].map(
  (handle) => ({
    id: `gid://shopify/Page/${handle}`,
    title: handle.replace(/-/g, " "),
    handle,
    body: `<p>${handle} page body.</p>`,
    bodySummary: `${handle} page`,
    createdAt: "2026-01-01T00:00:00Z",
    updatedAt: "2026-10-01T00:00:00Z",
  }),
);

export function emptyCart(id: string): Cart {
  return {
    id,
    checkoutUrl: "/checkout",
    totalQuantity: 0,
    lines: [],
    cost: { subtotalAmount: money("0.0"), totalAmount: money("0.0"), totalTaxAmount: money("0.0") },
  };
}

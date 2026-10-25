// tsx2rs test app: drop-in replacement for the Shopify Storefront client. Same exports and signatures,
// backed by local fixtures (lib/shopify/fixtures.ts) and an in-memory cart keyed by the `cartId` cookie.
// The original file is upstream vercel/commerce lib/shopify/index.ts.
import { TAGS } from "lib/constants";
import { revalidateTag } from "next/cache";
import { cookies } from "next/headers";
import { NextRequest, NextResponse } from "next/server";
import { collections, emptyCart, menus, pages, products } from "./fixtures";
import type { Cart, CartItem, Collection, Menu, Page, Product } from "./types";

const carts = new Map<string, Cart>();

// Simulated backend latency per call (ms), e.g. LATENCY_PRODUCT_MS=30. Default 0 (instant).
const latency = (name: string) => Number(process.env[`LATENCY_${name}_MS`] ?? 0);
const wait = (name: string) => {
  const ms = latency(name);
  return ms ? new Promise((r) => setTimeout(r, ms)) : undefined;
};

async function cartId(): Promise<string | undefined> {
  return (await cookies()).get("cartId")?.value;
}

function recalc(cart: Cart): Cart {
  const total = cart.lines.reduce((s, l) => s + Number(l.cost.totalAmount.amount), 0);
  cart.totalQuantity = cart.lines.reduce((s, l) => s + l.quantity, 0);
  cart.cost = {
    subtotalAmount: { amount: total.toFixed(1), currencyCode: "USD" },
    totalAmount: { amount: total.toFixed(1), currencyCode: "USD" },
    totalTaxAmount: { amount: "0.0", currencyCode: "USD" },
  };
  return cart;
}

export async function createCart(): Promise<Cart> {
  const id = `gid://shopify/Cart/${carts.size + 1}-${Date.now()}`;
  const cart = emptyCart(id);
  carts.set(id, cart);
  return cart;
}

export async function addToCart(lines: { merchandiseId: string; quantity: number }[]): Promise<Cart> {
  const id = (await cartId())!;
  const cart = carts.get(id) ?? emptyCart(id);
  for (const { merchandiseId, quantity } of lines) {
    const product = products.find((p) => p.variants.some((v) => v.id === merchandiseId))!;
    const variant = product.variants.find((v) => v.id === merchandiseId)!;
    const existing = cart.lines.find((l) => l.merchandise.id === merchandiseId);
    if (existing) {
      existing.quantity += quantity;
      existing.cost.totalAmount.amount = (Number(variant.price.amount) * existing.quantity).toFixed(1);
    } else {
      const line: CartItem = {
        id: `gid://shopify/CartLine/${cart.lines.length + 1}`,
        quantity,
        cost: { totalAmount: { amount: (Number(variant.price.amount) * quantity).toFixed(1), currencyCode: "USD" } },
        merchandise: {
          id: variant.id,
          title: variant.title,
          selectedOptions: variant.selectedOptions,
          product: { id: product.id, handle: product.handle, title: product.title, featuredImage: product.featuredImage },
        },
      };
      cart.lines.push(line);
    }
  }
  carts.set(id, recalc(cart));
  return cart;
}

export async function removeFromCart(lineIds: string[]): Promise<Cart> {
  const id = (await cartId())!;
  const cart = carts.get(id) ?? emptyCart(id);
  cart.lines = cart.lines.filter((l) => !lineIds.includes(l.id!));
  return recalc(cart);
}

export async function updateCart(lines: { id: string; merchandiseId: string; quantity: number }[]): Promise<Cart> {
  const id = (await cartId())!;
  const cart = carts.get(id) ?? emptyCart(id);
  for (const u of lines) {
    const line = cart.lines.find((l) => l.id === u.id);
    if (line) {
      const unit = Number(line.cost.totalAmount.amount) / line.quantity;
      line.quantity = u.quantity;
      line.cost.totalAmount.amount = (unit * u.quantity).toFixed(1);
    }
  }
  return recalc(cart);
}

export async function getCart(): Promise<Cart | undefined> {
  await wait("CART");
  const id = await cartId();
  if (!id) return undefined;
  return carts.get(id) ?? emptyCart(id);
}

export async function getCollection(handle: string): Promise<Collection | undefined> {
  return collections.find((c) => c.handle === handle);
}

export async function getCollectionProducts({
  collection,
}: {
  collection: string;
  reverse?: boolean;
  sortKey?: string;
}): Promise<Product[]> {
  return products.filter((p) => p.tags.includes(collection));
}

export async function getCollections(): Promise<Collection[]> {
  return [
    { handle: "", title: "All", description: "All products", seo: { title: "All", description: "All products" }, path: "/search", updatedAt: "2026-10-01T00:00:00Z" },
    ...collections,
  ];
}

export async function getMenu(handle: string): Promise<Menu[]> {
  await wait("MENU");
  return menus[handle] ?? [];
}

export async function getPage(handle: string): Promise<Page> {
  return pages.find((p) => p.handle === handle)!;
}

export async function getPages(): Promise<Page[]> {
  return pages;
}

export async function getProduct(handle: string): Promise<Product | undefined> {
  await wait("PRODUCT");
  return products.find((p) => p.handle === handle);
}

export async function getProductRecommendations(productId: string): Promise<Product[]> {
  await wait("RECOMMENDATIONS");
  return products.filter((p) => p.id !== productId).slice(0, 8);
}

export async function getProducts({ query }: { query?: string; reverse?: boolean; sortKey?: string }): Promise<Product[]> {
  const q = query?.toLowerCase();
  return q ? products.filter((p) => p.title.toLowerCase().includes(q)) : products;
}

export async function revalidate(req: NextRequest): Promise<NextResponse> {
  revalidateTag(TAGS.products, "max");
  return NextResponse.json({ status: 200, revalidated: true, now: Date.now() });
}

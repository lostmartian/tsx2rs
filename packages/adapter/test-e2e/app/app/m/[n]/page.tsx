import type { Metadata } from "next";

export const dynamic = "force-dynamic";

const NAMES: Record<string, string> = { "1": "One", "2": "Two", "3": "Three" };

// Metadata the param doesn't explain (as a product's title, from data): learned per request.
export async function generateMetadata({
  params,
}: {
  params: Promise<{ n: string }>;
}): Promise<Metadata> {
  const { n } = await params;
  return { title: NAMES[n] ?? "Unknown", description: `About ${NAMES[n] ?? n}` };
}

export default async function Page({ params }: { params: Promise<{ n: string }> }) {
  const { n } = await params;
  return <h1>Page {n}</h1>;
}

import type { Metadata } from "next";

export const dynamic = "force-dynamic";

// Metadata made from the param alone: the wrapper explains it, so any request is served.
export async function generateMetadata({
  params,
}: {
  params: Promise<{ n: string }>;
}): Promise<Metadata> {
  const { n } = await params;
  return { title: `Item ${n}` };
}

export default async function Page({ params }: { params: Promise<{ n: string }> }) {
  const { n } = await params;
  return <h1>Item {n}</h1>;
}

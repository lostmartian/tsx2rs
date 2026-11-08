import { Suspense } from "react";
import { Counter } from "../../components/counter";
import { Price } from "../../components/price";

export const dynamic = "force-dynamic";

// Two Suspense boundaries: whether React leaves each pending is learned from Next.js's response.
export default async function Page({ params }: { params: Promise<{ n: string }> }) {
  const { n } = await params;
  return (
    <main>
      <h1>Boundaries {n}</h1>
      <Suspense fallback={<p>Loading the price</p>}>
        <Price amount={Number(n)} />
      </Suspense>
      <Suspense fallback={<p>Loading the counter</p>}>
        <Counter />
      </Suspense>
    </main>
  );
}

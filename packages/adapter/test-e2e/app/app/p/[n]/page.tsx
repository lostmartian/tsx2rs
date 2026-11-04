import { Counter } from "../../components/counter";
import { Price } from "../../components/price";

export const dynamic = "force-dynamic";

export default async function Page({ params }: { params: Promise<{ n: string }> }) {
  const { n } = await params;
  return (
    <main>
      <h1>Product {n}</h1>
      <Price amount={Number(n)} />
      <Counter />
    </main>
  );
}

import { Price } from "../../components/price";

export const dynamic = "force-dynamic";

// No client components: the whole page encodes natively.
export default async function Page({ params }: { params: Promise<{ n: string }> }) {
  const { n } = await params;
  return (
    <main>
      <h1>Price {n}</h1>
      <Price amount={Number(n)} currency="EUR" />
    </main>
  );
}

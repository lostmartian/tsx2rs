import { Suspense } from "react";
import { Counter } from "./counter";

/** Data that arrives late, so the boundary below streams after the shell. */
async function Recommendations() {
  await new Promise((resolve) => setTimeout(resolve, 100));
  const names = ["One", "Two", "Three"];
  return (
    <section className="related">
      <h2>Related</h2>
      <ol>
        {names.map((name) => (
          <li key={name}>{name}</li>
        ))}
      </ol>
    </section>
  );
}

export const dynamic = "force-dynamic";

export default function Page() {
  return (
    <main>
      <h1>Product</h1>
      <Counter label="Add to cart" start={2} />
      <Suspense fallback={<div className="skeleton" />}>
        <Recommendations />
      </Suspense>
      <footer>Footer</footer>
    </main>
  );
}

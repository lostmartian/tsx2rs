// Static markup folded at build time, with text and number holes between static text.
export default function Page({ name, count }: { name: string; count: number }) {
  return (
    <main className="page">
      <h1>Hello, {name}!</h1>
      <p>
        You have {count} new {count === 1 ? "message" : "messages"}.
      </p>
      {"literal"}
      {42}
    </main>
  );
}

export const cases = [
  { name: "Ada", count: 1 },
  { name: "<script>&\"'", count: 0 },
  { name: "", count: -2.5 },
];

// `.map` lists: keys (React escapes `$` as `$$`), empty lists, items that read nothing, and the
// component's own chain before `.map`.
interface Item {
  id: string | number;
  name: string;
  ok: boolean;
}
export default function Page({ items }: { items: Item[] }) {
  return (
    <section>
      <ul>
        {items.map((item) => (
          <li key={item.id}>{item.name}</li>
        ))}
      </ul>
      <ol>
        {items
          .filter((item) => item.ok)
          .map((item, i) => (
            <li key={item.id}>
              {i}: {item.name}
            </li>
          ))}
      </ol>
      {items.map(() => (
        <hr />
      ))}
    </section>
  );
}

export const cases = [
  { items: [] },
  {
    items: [
      { id: 1, name: "one", ok: true },
      { id: "$b", name: "dollar", ok: false },
      { id: "c", name: "<three>", ok: true },
    ],
  },
];

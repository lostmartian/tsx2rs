// A component that renders itself: one template, run with `Call`.
interface Menu {
  label: string;
  items: Menu[];
}
function Tree({ menu }: { menu: Menu }) {
  return (
    <li>
      {menu.label}
      {menu.items.length > 0 && (
        <ul>
          {menu.items.map((item) => (
            <Tree key={item.label} menu={item} />
          ))}
        </ul>
      )}
    </li>
  );
}
export default function Page({ menu }: { menu: Menu }) {
  return (
    <nav>
      <ul>
        <Tree menu={menu} />
      </ul>
    </nav>
  );
}

export const cases = [
  { menu: { label: "root", items: [] } },
  {
    menu: {
      label: "root",
      items: [
        { label: "a", items: [{ label: "a1", items: [] }] },
        { label: "b", items: [] },
      ],
    },
  },
];

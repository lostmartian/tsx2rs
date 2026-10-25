const items = [
  { id: "a", name: "Alpha", price: 1.5, inStock: true },
  { id: "b", name: "Beta & <Gamma>", price: 20, inStock: false },
  { id: "c", name: 'Quote "and" apostrophe\'s', price: 0, inStock: true },
];

export const dynamic = "force-dynamic";

export default function Page() {
  const total = items.reduce((sum, item) => sum + item.price, 0);
  const jsonLd = {
    "@context": "https://schema.org",
    "@type": "ItemList",
    numberOfItems: items.length,
  };
  return (
    <main className="catalog" data-count={items.length}>
      <h1>
        Items: {items.length} in stock: {items.filter((i) => i.inStock).length}
      </h1>
      <script
        type="application/ld+json"
        dangerouslySetInnerHTML={{ __html: JSON.stringify(jsonLd) }}
      />
      <ul style={{ listStyle: "none", paddingLeft: 0, marginTop: 8 }}>
        {items.map((item) => (
          <li key={item.id} className={item.inStock ? "in-stock" : undefined}>
            {item.name} costs {item.price}
            {item.inStock ? null : <em> (sold out)</em>}
            <input type="checkbox" readOnly checked={item.inStock} disabled={!item.inStock} />
          </li>
        ))}
      </ul>
      <p>
        Total: {total}
        <br />
        {""}
        {false}
        {0}
      </p>
    </main>
  );
}

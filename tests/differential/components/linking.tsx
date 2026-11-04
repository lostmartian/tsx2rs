// Linked child components: props as JSX builds them, keyed children in a list (React's key
// path), a child whose root has its own key (React's implicit slot), a child returning text,
// and a child with several returns.
function Price({ amount, currency = "USD" }: { amount: number; currency?: string }) {
  const text = new Intl.NumberFormat("en-US", { style: "currency", currency }).format(amount);
  return (
    <p className="price">
      {text}
      <span>{currency}</span>
    </p>
  );
}
function Badge({ label }: { label: string }) {
  return <b key="badge">{label}</b>;
}
function Name({ name }: { name: string }) {
  return name;
}
function Maybe({ show }: { show: boolean }) {
  if (!show) return null;
  return <em>shown</em>;
}
function Row({ label }: { label: string }) {
  return (
    <li>
      <Badge label={label} />
    </li>
  );
}
export default function Page({
  amount,
  names,
  show,
}: {
  amount: number;
  names: string[];
  show: boolean;
}) {
  return (
    <main>
      <Price amount={amount} />
      <Price amount={amount * 2} currency="EUR" />
      <Badge label="alone" />
      <p>
        Hi <Name name={names[0] ?? "nobody"} />!
      </p>
      <Maybe show={show} />
      <ul>
        {names.map((name) => (
          <Row key={name} label={name} />
        ))}
      </ul>
    </main>
  );
}

export const cases = [
  { amount: 12.5, names: ["ada", "$bob"], show: true },
  { amount: 0, names: [], show: false },
];

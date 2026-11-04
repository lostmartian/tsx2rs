// Keys: a keyed element returned through a conditional at a component's root (React's key path
// and implicit slot), a host element's key after a spread (`createElement`, evaluated in
// place), a dynamic key without a spread (evaluated after the props), and a `key` inside a
// spread on a host element (React takes it as the key).
let log: string[] = [];
const seen = (name: string, value: string) => {
  log.push(name);
  return value;
};
function Pick({ a }: { a: boolean }) {
  return a ? <b key="picked">yes</b> : <i>no</i>;
}
function Report() {
  const order = log.join(",");
  log = [];
  return <p>{order}</p>;
}
export default function Page({ a, rest }: { a: boolean; rest: Record<string, unknown> }) {
  log = [];
  return (
    <div>
      <Pick a={a} />
      <Pick key="outer" a={a} />
      <ul>
        {["x", "y"].map((k) => (
          <li title={seen("title " + k, k)} {...rest} key={seen("key " + k, k)}>
            {k}
          </li>
        ))}
      </ul>
      <span {...rest}>spread</span>
      {["p", "q"].map((k) => (
        <em title={seen("em title " + k, k)} key={seen("em key " + k, k)}>
          {k}
        </em>
      ))}
      <Report />
    </div>
  );
}

export const cases = [
  { a: true, rest: {} },
  { a: false, rest: { id: "r" } },
  { a: true, rest: { key: "from-spread", id: "r" } },
];

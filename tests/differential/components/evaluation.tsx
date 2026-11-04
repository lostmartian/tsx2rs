// Evaluation order: the data function runs the component's own code, each expression once, in
// JSX's order (a key after a spread is evaluated in place; children run after their parent).
let log: string[] = [];
const seen = (name: string, value: string) => {
  log.push(name);
  return value;
};
function Child({ v }: { v: string }) {
  return <i>{seen("child body", v)}</i>;
}
function Report() {
  const order = log.join(",");
  log = [];
  return <p>{order}</p>;
}
export default function Page({ rest }: { rest: Record<string, unknown> }) {
  log = [];
  return (
    <div>
      <Child v={seen("prop a", "a")} {...rest} key={seen("key a", "ka")} />
      <Child key={seen("key b", "kb")} v={seen("prop b", "b")} />
      {seen("hole", "h")}
      {seen("falsy test", "") && <b>never</b>}
      {seen("nullish test", "kept") ?? <b>never</b>}
      <Report />
    </div>
  );
}

export const cases = [{ rest: {} }, { rest: { v: "spread" } }];

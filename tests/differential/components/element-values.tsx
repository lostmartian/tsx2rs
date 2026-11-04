// Element values (ADR-0007): JSX the template keeps as React elements, which `Op::Child` reads
// as element nodes: special elements, `dangerouslySetInnerHTML`, `<svg>`, and JSX built in
// setup code.
export default function Page({
  v,
  html,
  d,
  choice,
  fresh,
}: {
  v: string;
  html: string;
  d: string;
  choice: string;
  fresh: boolean;
}) {
  const badge = fresh ? <em className="badge">new</em> : null;
  return (
    <form>
      <input name="q" value={v} readOnly />
      <textarea defaultValue={v} />
      <select value={choice} onChange={undefined}>
        <option value="a">A</option>
        <option value="b">B {v}</option>
      </select>
      <div dangerouslySetInnerHTML={{ __html: html }} />
      <svg viewBox="0 0 10 10">
        <path d={d} />
      </svg>
      <p>{badge}</p>
    </form>
  );
}

export const cases = [
  { v: "x<y", html: "<b>bold</b>", d: "M0 0L10 10", choice: "b", fresh: true },
  { v: "", html: "", d: "", choice: "a", fresh: false },
];

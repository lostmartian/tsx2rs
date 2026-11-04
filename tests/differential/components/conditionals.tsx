// The four conditionals, with React's "render the tested value" cases: `&&` renders a falsy
// test (`0`, `""`, `false`), `||` a truthy one, and `??` anything but null or undefined.
import type { ReactNode } from "react";

export default function Page({ a, b, c }: { a: ReactNode; b: ReactNode; c: ReactNode }) {
  return (
    <div>
      {a ? <b>yes</b> : <i>no</i>}
      {a && <span>and</span>}
      {b || <span>or</span>}
      {c ?? <span>nullish</span>}
      {a ? "text" : null}
    </div>
  );
}

export const cases = [
  { a: 1, b: "kept", c: 0 },
  { a: 0, b: "", c: null },
  { a: "", b: false, c: undefined },
  { a: false, b: 0, c: false },
  { a: true, b: true, c: "" },
];

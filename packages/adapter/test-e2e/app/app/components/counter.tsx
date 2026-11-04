"use client";
import { useState } from "react";
import { Price } from "./price";

// A client component that renders `Price` too: `price.tsx` is in the client bundle as well as the
// server's, and only the server's copy may get data functions.
export function Counter() {
  const [n, setN] = useState(0);
  return (
    <button
      type="button"
      onClick={() => {
        setN(n + 1);
      }}
    >
      <Price amount={n} />
    </button>
  );
}

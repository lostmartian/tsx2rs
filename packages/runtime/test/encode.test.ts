import { createElement } from "react";
import { describe, expect, it } from "vitest";
import {
  ENTRY_KEYS,
  Input,
  NO_KEYS,
  NotNative,
  attrs,
  call,
  node,
  primitive,
  reference,
  rootKey,
  style,
  value,
  type Keys,
} from "../src/encode.js";

/** The bytes `write` produces. */
function bytes(write: (w: Input) => void): number[] {
  const w = new Input();
  write(w);
  return [...w.finish()];
}

const u32 = (n: number) => [n & 0xff, (n >>> 8) & 0xff, (n >>> 16) & 0xff, n >>> 24];
const str = (s: string) => {
  const b = [...new TextEncoder().encode(s)];
  return [...u32(b.length), ...b];
};
const f64 = (n: number) => {
  const b = new Uint8Array(8);
  new DataView(b.buffer).setFloat64(0, n, true);
  return [...b];
};
const T = {
  undefined: 0,
  null: 1,
  false: 2,
  true: 3,
  number: 4,
  string: 5,
  array: 6,
  object: 7,
  bigint: 8,
  element: 9,
  reference: 11,
};

describe("primitive", () => {
  it("tags every primitive", () => {
    expect(
      bytes((w) => {
        primitive(w, undefined);
        primitive(w, null);
        primitive(w, false);
        primitive(w, true);
      }),
    ).toEqual([T.undefined, T.null, T.false, T.true]);
    expect(
      bytes((w) => {
        primitive(w, -0.5);
      }),
    ).toEqual([T.number, ...f64(-0.5)]);
    expect(
      bytes((w) => {
        primitive(w, "é<");
      }),
    ).toEqual([T.string, ...str("é<")]);
    expect(
      bytes((w) => {
        primitive(w, 2n ** 70n);
      }),
    ).toEqual([T.bigint, ...str("1180591620717411303424")]);
  });

  it("refuses what isn't a primitive", () => {
    expect(() =>
      bytes((w) => {
        primitive(w, {});
      }),
    ).toThrow(NotNative);
    expect(() =>
      bytes((w) => {
        primitive(w, () => 1);
      }),
    ).toThrow(NotNative);
    expect(() =>
      bytes((w) => {
        primitive(w, Symbol("s"));
      }),
    ).toThrow(NotNative);
  });
});

describe("Input", () => {
  it("grows past its first buffer and writes UTF-8 lengths in bytes", () => {
    const long = "€".repeat(1000); // 3 bytes each
    expect(
      bytes((w) => {
        w.string(long);
      }),
    ).toEqual(str(long));
    expect(
      bytes((w) => {
        for (let i = 0; i < 300; i++) w.u32(i);
      }),
    ).toHaveLength(1200);
  });

  it("writes lone surrogates as U+FFFD, as UTF-8 requires", () => {
    expect(
      bytes((w) => {
        w.string("\ud800");
      }),
    ).toEqual(str("�"));
  });
});

describe("style and value", () => {
  it("writes a style object's entries as primitives", () => {
    expect(
      bytes((w) => {
        style(w, { color: "red", marginTop: 0 });
      }),
    ).toEqual([
      T.object,
      ...u32(2),
      ...str("color"),
      T.string,
      ...str("red"),
      ...str("marginTop"),
      T.number,
      ...f64(0),
    ]);
    expect(
      bytes((w) => {
        style(w, null);
      }),
    ).toEqual([T.null]);
    expect(() =>
      bytes((w) => {
        style(w, "color: red");
      }),
    ).toThrow(NotNative);
  });

  it("writes arrays and plain objects, and refuses other objects", () => {
    expect(
      bytes((w) => {
        value(w, [1, { a: null }]);
      }),
    ).toEqual([T.array, ...u32(2), T.number, ...f64(1), T.object, ...u32(1), ...str("a"), T.null]);
    expect(
      bytes((w) => {
        value(w, Object.create(null) as object);
      }),
    ).toEqual([T.object, ...u32(0)]);
    expect(() =>
      bytes((w) => {
        value(w, new Date(0));
      }),
    ).toThrow(NotNative);
    expect(() =>
      bytes((w) => {
        value(w, new Map());
      }),
    ).toThrow(NotNative);
  });

  it("writes Flight references with their own tag, and nothing else as one", () => {
    expect(
      bytes((w) => {
        reference(w, "$L15");
      }),
    ).toEqual([T.reference, ...str("$L15")]);
    for (const ok of ["$3", "$@a", "$a"]) {
      expect(
        bytes((w) => {
          reference(w, ok);
        })[0],
      ).toBe(T.reference);
    }
    for (const bad of ["L15", "$", "$Lx", "$LL1", "$L1A"]) {
      expect(() => {
        reference(new Input(), bad);
      }).toThrow("not a Flight reference");
    }
  });
});

describe("attrs", () => {
  it("writes props in order, style as a style value", () => {
    expect(
      bytes((w) => {
        attrs(w, { id: "a", style: { color: "red" }, hidden: true });
      }),
    ).toEqual([
      ...u32(3),
      ...str("id"),
      T.string,
      ...str("a"),
      ...str("style"),
      T.object,
      ...u32(1),
      ...str("color"),
      T.string,
      ...str("red"),
      ...str("hidden"),
      T.true,
    ]);
    expect(() =>
      bytes((w) => {
        attrs(w, new Map());
      }),
    ).toThrow(NotNative);
  });
});

describe("node", () => {
  it("writes host elements as element nodes, children last", () => {
    const element = createElement("a", { href: "/x", key: "k" }, "go");
    expect(
      bytes((w) => {
        node(w, [element, 1]);
      }),
    ).toEqual([
      T.array,
      ...u32(2),
      T.element,
      ...str("a"),
      T.string,
      ...str("k"),
      ...u32(2),
      ...str("href"),
      T.string,
      ...str("/x"),
      ...str("children"),
      T.string,
      ...str("go"),
      T.number,
      ...f64(1),
    ]);
  });

  it("writes an element's dangerouslySetInnerHTML and array props as values", () => {
    const element = createElement("select", {
      value: ["a"],
      dangerouslySetInnerHTML: { __html: "<b>" },
    });
    expect(
      bytes((w) => {
        node(w, element);
      }),
    ).toEqual([
      T.element,
      ...str("select"),
      T.null,
      ...u32(2),
      ...str("value"),
      T.array,
      ...u32(1),
      T.string,
      ...str("a"),
      ...str("dangerouslySetInnerHTML"),
      T.object,
      ...u32(1),
      ...str("__html"),
      T.string,
      ...str("<b>"),
    ]);
  });

  it("hands React what the engine doesn't render natively", () => {
    const component = () => null;
    for (const v of [
      createElement(component),
      Promise.resolve("x"),
      new Set([1]),
      { a: 1 },
      () => "f",
      Symbol("s"),
      createElement("button", { onClick: () => undefined }),
    ]) {
      expect(() =>
        bytes((w) => {
          node(w, v);
        }),
      ).toThrow(NotNative);
    }
  });
});

describe("keys", () => {
  it("puts the key path in front of a root element's key", () => {
    expect(rootKey(ENTRY_KEYS, "k")).toBe("k");
    expect(rootKey({ path: "a,b", implicit: false }, null)).toBe("a,b");
    expect(rootKey({ path: "a", implicit: false }, "k")).toBe("a,k");
  });

  it("resolves a pending call as React's jsx does, and gives the callee its key path", () => {
    const seen: [Record<string, unknown>, Keys][] = [];
    const run = (outer: Keys, pending: unknown) => {
      bytes((w) => {
        call(
          w,
          outer,
          pending,
          (props) => [props],
          (_w, record, keys) => {
            seen.push([record[0] as Record<string, unknown>, keys]);
          },
        );
      });
      return seen.at(-1);
    };
    // A key argument, stringified; a key inside the config wins and isn't a prop.
    expect(run(ENTRY_KEYS, [{ a: 1 }, 7])).toEqual([{ a: 1 }, { path: "7", implicit: true }]);
    expect(run(ENTRY_KEYS, [{ key: "in", a: 1 }, "arg"])).toEqual([
      { a: 1 },
      { path: "in", implicit: true },
    ]);
    expect(run(ENTRY_KEYS, [{ key: undefined }, "arg"])).toEqual([
      {},
      { path: "arg", implicit: true },
    ]);
    // Unkeyed: the path passes through; with none, the output is an implicit slot.
    expect(run({ path: "p", implicit: false }, [{}, undefined])).toEqual([
      {},
      { path: "p", implicit: false },
    ]);
    expect(run(NO_KEYS, [{}, undefined])).toEqual([{}, { path: null, implicit: true }]);
    expect(run({ path: "p", implicit: false }, [{}, "k"])).toEqual([
      {},
      { path: "p,k", implicit: false },
    ]);
  });
});

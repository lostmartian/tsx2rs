import fc from "fast-check";
import { describe, expect, it } from "vitest";
import { decodeFlight, extractFlight, IdentityError, parseRows } from "../src/index.ts";

const bytes = (s: string) => new TextEncoder().encode(s);
const push = (kind: number, data?: string) =>
  `<script>self.__next_f.push(${JSON.stringify(data === undefined ? [kind] : [kind, data])})</script>`;
const decode = (payload: string) => decodeFlight(bytes(payload));

function codeOf(fn: () => unknown): string | undefined {
  try {
    fn();
  } catch (e) {
    if (e instanceof IdentityError) return e.code;
    throw e;
  }
  return undefined;
}

describe("extractFlight", () => {
  it("concatenates text and base64 pushes in order and keeps other scripts in the HTML", () => {
    const html =
      "<body>" +
      push(0) +
      push(1, '0:"a') +
      push(3, Buffer.from('b"\n').toString("base64")) +
      push(2, "form") +
      '<script nonce="n1">self.__next_f.push([1,"1:2\\n"])</script>' +
      "</body>";
    const { payload, html: rest } = extractFlight(html);
    expect(new TextDecoder().decode(payload)).toBe('0:"ab"\n1:2\n');
    expect(rest).toBe("<body>" + push(0) + push(2, "form") + "</body>");
  });

  it("returns an empty payload for a document without Flight", () => {
    expect(extractFlight("<p>x</p>")).toEqual({ payload: new Uint8Array(), html: "<p>x</p>" });
  });
});

describe("parseRows", () => {
  it("reads model, module, hint, error and text rows", () => {
    const text = "héllo ✓\nwith a newline";
    const t = `2:T${Buffer.byteLength(text).toString(16)},${text}`;
    const rows = parseRows(
      bytes(
        `0:["$","p",null,{}]\n1:I["m1",[],"Button"]\n:HL["/a.css","style"]\n${t}3:E{"digest":"x"}\n`,
      ),
    );
    expect([...rows.byId]).toEqual([
      ["0", { kind: "model", json: '["$","p",null,{}]' }],
      ["1", { kind: "module", json: '["m1",[],"Button"]' }],
      ["2", { kind: "text", text }],
      ["3", { kind: "error", json: '{"digest":"x"}' }],
    ]);
    expect(rows.hints).toEqual([["L", '["/a.css","style"]']]);
  });

  it("collects a stream's items until it is closed (X … C, as Next.js metadata sends)", () => {
    const rows = parseRows(bytes('7:X\n7:{"a":1}\n7:2\n7:C\n'));
    expect(rows.byId.get("7")).toEqual({
      kind: "stream",
      stream: "asyncIterable",
      items: ['{"a":1}', "2"],
      close: "",
    });
  });

  it.each([
    ["a duplicate row id", "0:1\n0:2\n"],
    ["a row id that isn't hex", "zz:1\n"],
    ["a row without ':'", "0\n"],
    ["a truncated text row", "0:T10,short"],
    ["a text row with a bad length", "0:Tzz,x"],
    ["a close without an open stream", "4:C\n"],
    ["a second close", "4:X\n4:C\n4:C\n"],
  ])("rejects %s", (_name, payload) => {
    expect(codeOf(() => parseRows(bytes(payload)))).toBe("MALFORMED_FLIGHT");
  });

  it.each([
    ["binary rows", "0:A4,abcd"],
    ["development rows", "0:D{}\n"],
    ["unknown row types", "0:P{}\n"],
  ])("refuses %s", (_name, payload) => {
    expect(codeOf(() => parseRows(bytes(payload)))).toBe("UNSUPPORTED_FLIGHT_ROW");
  });
});

describe("decodeFlight", () => {
  it("decodes elements and React's special values", () => {
    const row0 = JSON.stringify([
      "$",
      "div",
      "k",
      {
        a: "$$literal",
        b: "$Sreact.suspense",
        c: "$undefined",
        d: "$D2026-10-25T00:00:00.000Z",
        e: "$n123",
        f: ["$Infinity", "$-Infinity", "$NaN", "$-0"],
        g: "$Z",
        h: 1.5,
        i: true,
        j: null,
      },
    ]);
    expect(decode(`0:${row0}\n`).root).toEqual({
      $$identity: "element",
      value: {
        type: "div",
        key: "k",
        props: {
          a: "$literal",
          b: { $$identity: "symbol", value: "react.suspense" },
          c: { $$identity: "undefined", value: null },
          d: { $$identity: "date", value: "2026-10-25T00:00:00.000Z" },
          e: { $$identity: "bigint", value: "123" },
          f: [
            { $$identity: "number", value: "Infinity" },
            { $$identity: "number", value: "-Infinity" },
            { $$identity: "number", value: "NaN" },
            { $$identity: "number", value: "-0" },
          ],
          g: { $$identity: "error", value: null },
          h: 1.5,
          i: true,
          j: null,
        },
      },
    });
  });

  it("resolves lazy and plain references transparently, and tags promises, maps and sets", () => {
    const flight = decode(
      [
        '0:{"lazy":"$L1","plain":"$1","promise":"$@1","map":"$Q2","set":"$W3","client":"$L4","text":"$5"}',
        '1:{"v":1}',
        '2:[["k","v"]]',
        "3:[1,2]",
        '4:I["m",["c.js"],"Button"]',
        // A text row ends after its byte length; no newline follows it.
        "5:T3,abc",
      ].join("\n"),
    );
    expect(flight.root).toEqual({
      lazy: { v: 1 },
      plain: { v: 1 },
      promise: { $$identity: "promise", value: { v: 1 } },
      map: { $$identity: "map", value: [["k", "v"]] },
      set: { $$identity: "set", value: [1, 2] },
      client: { $$identity: "clientReference", value: ["m", ["c.js"], "Button"] },
      text: "abc",
    });
  });

  it("gives the same tree whether content is inline or split into rows", () => {
    const inline = decode('0:["$","main",null,{"children":["$","p",null,{"children":"hi"}]}]\n');
    const split = decode(
      '0:["$","main",null,{"children":"$L1"}]\n1:["$","p",null,{"children":"hi"}]\n',
    );
    expect(split).toEqual(inline);
  });

  it("follows React's deduplication paths, including into the row being decoded", () => {
    const flight = decode(
      [
        '0:["$","div",null,{"a":{"opts":[1,2]},"b":"$0:props:a:opts","c":"$1:props:children:0"}]',
        '1:["$","ul",null,{"children":["$L2"]}]',
        '2:{"x":"y"}',
      ].join("\n") + "\n",
    );
    expect(flight.root).toMatchObject({
      value: { props: { a: { opts: [1, 2] }, b: [1, 2], c: { x: "y" } } },
    });
  });

  it('keeps a "__proto__" key as data instead of changing the object\'s prototype', () => {
    const root = decode('0:{"__proto__":{"polluted":true},"a":1}\n').root as Record<
      string,
      unknown
    >;
    expect(Object.keys(root)).toEqual(["__proto__", "a"]);
    expect(Object.getPrototypeOf(root)).toBe(Object.prototype);
    expect(Object.getOwnPropertyDescriptor(root, "__proto__")?.value).toEqual({ polluted: true });
  });

  it("follows a deduplication path through a plain row reference", () => {
    expect(decode('0:"$2:a:x"\n1:{"x":5}\n2:{"a":"$1"}\n').root).toBe(5);
  });

  it("decodes hints as plain JSON, where `$` means nothing", () => {
    const flight = decode(':HL["$","image"]\n:HL["$1",{"x":"$@2"}]\n0:null\n');
    expect(flight.hints).toEqual([
      ["L", ["$", "image"]],
      ["L", ["$1", { x: "$@2" }]],
    ]);
  });

  it("decodes hints and a stream's items and close value", () => {
    const flight = decode(':HL["/f.woff2","font"]\n0:{"meta":"$@7"}\n7:X\n7:{"t":"a"}\n7:C\n');
    expect(flight.hints).toEqual([["L", ["/f.woff2", "font"]]]);
    expect(flight.root).toEqual({
      meta: {
        $$identity: "promise",
        value: {
          $$identity: "stream",
          value: {
            type: "asyncIterable",
            items: [{ t: "a" }],
            close: { $$identity: "undefined", value: null },
          },
        },
      },
    });
  });

  it("decodes a stream's explicit close value and an unclosed stream", () => {
    expect(decode('0:["$1","$2"]\n1:R\n1:C"done"\n2:x\n').root).toEqual([
      { $$identity: "stream", value: { type: "readable", items: [], close: "done" } },
      {
        $$identity: "stream",
        value: {
          type: "asyncIterator",
          items: [],
          close: { $$identity: "undefined", value: null },
        },
      },
    ]);
  });

  it("decodes error rows and server references", () => {
    expect(decode('0:["$1","$h2"]\n1:E{"digest":"d"}\n2:{"id":"a","bound":null}\n').root).toEqual([
      { $$identity: "error", value: { digest: "d" } },
      { $$identity: "serverReference", value: { id: "a", bound: null } },
    ]);
  });

  it("round-trips any JSON value, escaping strings that start with $ (property)", () => {
    const escape = (v: unknown): unknown => {
      if (typeof v === "string") return v.startsWith("$") ? `$${v}` : v;
      if (Array.isArray(v)) return v.map(escape);
      if (v !== null && typeof v === "object") {
        return Object.fromEntries(Object.entries(v).map(([k, x]) => [k, escape(x)]));
      }
      return v;
    };
    fc.assert(
      fc.property(fc.jsonValue(), (value) => {
        // `["$", …]` with four or more items is an element; plain data never takes that shape
        // because React escapes the "$" string to "$$".
        const payload = `0:${JSON.stringify(escape(value))}\n`;
        expect(decode(payload).root).toEqual(JSON.parse(JSON.stringify(value)));
      }),
    );
  });

  it.each([
    ["a missing row", '0:"$9"\n', "DANGLING_FLIGHT_REFERENCE"],
    ["a missing path", '0:{"a":"$1:nope"}\n1:{}\n', "DANGLING_FLIGHT_REFERENCE"],
    ["a reference cycle", '0:{"self":"$0"}\n', "CYCLIC_FLIGHT_REFERENCE"],
    ["a temporary reference", '0:"$T1"\n', "UNSUPPORTED_FLIGHT_VALUE"],
    ["a bare element marker", '0:["$"]\n', "MALFORMED_FLIGHT"],
    ["invalid JSON", "0:{oops}\n", "MALFORMED_FLIGHT"],
    ["a path into a text row", '0:"$1:x"\n1:T1,a', "MALFORMED_FLIGHT"],
    ["a path through a text row", '0:"$2:a:b"\n1:T1,a2:{"a":"$1"}\n', "MALFORMED_FLIGHT"],
    ["a lazy reference without a row id", '0:"$L"\n', "MALFORMED_FLIGHT"],
  ])("refuses %s", (_name, payload, code) => {
    expect(codeOf(() => decode(payload))).toBe(code);
  });
});

import { readFileSync } from "node:fs";
import { join } from "node:path";
import { flightScripts } from "@tsx2rs/native";
import { describe, expect, it } from "vitest";
import { confirm, confirmed, fill, learn, type Sample, type Template } from "../src/capture.js";

/**
 * Real responses from Next.js 16.4.5 (`next start`) for the adapter's test page
 * `app/q/[n]/page.tsx` (packages/adapter/test-e2e/app); see test/next/README.md.
 */
const doc = (name: string) =>
  readFileSync(join(import.meta.dirname, "next", `${name}.html`), "utf8");

/** What the engine renders for the page with `n` (as React does: the fixtures prove it). */
function page(n: string) {
  const price = Number(n).toFixed(2);
  const html = `<main><h1>Price <!-- -->${n}</h1><p class="price">${price}<!-- --> <span>EUR</span></p></main>`;
  const flight = JSON.stringify([
    "$",
    "main",
    null,
    {
      children: [
        ["$", "h1", null, { children: ["Price ", n] }],
        [
          "$",
          "p",
          null,
          { className: "price", children: [price, " ", ["$", "span", null, { children: "EUR" }]] },
        ],
      ],
    },
  ]);
  return { name: "page", html, flight };
}

function sample(name: string, n: string, search = ""): Sample {
  return { document: doc(name), params: { n }, pathname: `/q/${n}`, search, fragments: [page(n)] };
}

const values = (s: Sample) => ({
  params: s.params,
  pathname: s.pathname,
  search: s.search,
  fragments: Object.fromEntries(s.fragments.map((f) => [f.name, f])),
});

function learned(s: Sample): Template {
  const result = learn(s);
  if (!result.ok) throw new Error(result.reason);
  return result.template;
}

const q3 = sample("q-3", "3");
const q7 = sample("q-7", "7");
const long = sample("q-long", "1234567890123456789012345678901234567890");
const escaped = sample("q-escaped", "a%20b%26c");
const query = sample("q-3-query", "3", "?x=1&y=%3Cb%3E");

describe("learn", () => {
  it("reproduces the response it learned from, byte for byte", () => {
    expect(fill(learned(q3), values(q3)).toString()).toBe(q3.document);
  });

  it("can't fill other param values until a response confirms them", () => {
    const template = learned(q3);
    expect(confirmed(template)).toBe(false);
    expect(() => fill(template, values(q7))).toThrow("param n isn't confirmed");
  });

  it("refuses a fragment that isn't in the response exactly once", () => {
    const wrong = { ...q3, fragments: [{ ...page("3"), html: "<main>not this page</main>" }] };
    expect(learn(wrong)).toEqual({
      ok: false,
      reason: "page's html found 0 times in Next.js's response, not once",
    });
    const twice = { ...q3, document: q3.document.replace("<body>", `<body>${page("3").html}`) };
    expect(learn(twice)).toMatchObject({
      ok: false,
      reason: "page's html found 2 times in Next.js's response, not once",
    });
  });

  it("refuses Flight scripts it can't reproduce", () => {
    const binary = {
      ...q3,
      document: q3.document.replace(
        "<body>",
        '<body><script>self.__next_f.push([3,"AAAA"])</script>',
      ),
    };
    expect(learn(binary)).toMatchObject({
      ok: false,
      reason: expect.stringContaining("binary Flight data") as string,
    });
    const odd = {
      ...q3,
      document: q3.document.replace(
        "<body>",
        '<body><script>self.__next_f.push([1, "x"])</script>',
      ),
    };
    expect(learn(odd)).toMatchObject({
      ok: false,
      reason: expect.stringContaining("not encoded as Next.js") as string,
    });
  });
});

describe("confirm", () => {
  it("turns the param's places into slots, so new values of any length fill exactly", () => {
    const result = confirm(learned(q3), q7);
    if (!result.ok) throw new Error(result.reason);
    expect(confirmed(result.template)).toBe(true);
    expect(fill(result.template, values(q7)).toString()).toBe(q7.document);
    expect(fill(result.template, values(q3)).toString()).toBe(q3.document);
    // Never seen: another length, and characters that are escaped where they appear.
    expect(fill(result.template, values(long)).toString()).toBe(long.document);
    expect(fill(result.template, values(escaped)).toString()).toBe(escaped.document);
  });

  it("fills the same bytes from fragments the engine gives as bytes (ADR-0031)", () => {
    const result = confirm(learned(q3), q7);
    if (!result.ok) throw new Error(result.reason);
    // Characters both HTML and the Flight script escape, and some that aren't escaped.
    const tricky = { ...page("7"), html: "<p>a&amp;b é ✓</p>", flight: '["</script>&",\u2028"é"]' };
    const asBytes = {
      ...tricky,
      bytes: {
        html: Buffer.from(tricky.html),
        script: flightScripts([Buffer.from(tricky.flight)])[0] ?? Buffer.alloc(0),
      },
    };
    const filled = (fragment: typeof tricky) =>
      fill(result.template, { ...values(q7), fragments: { page: fragment } });
    expect(filled(asBytes).equals(filled(tricky))).toBe(true);
    expect(filled(asBytes).toString()).toContain("\\u003c/script\\u003e\\u0026");
  });

  it("fills the URL, query string included, where Next.js's router state has it", () => {
    const result = confirm(learned(q3), q7);
    if (!result.ok) throw new Error(result.reason);
    expect(fill(result.template, values(query)).toString()).toBe(query.document);
  });

  it("keeps a candidate when the other response has the same value", () => {
    const result = confirm(learned(q3), sample("q-3", "3"));
    if (!result.ok) throw new Error(result.reason);
    expect(confirmed(result.template)).toBe(false);
  });

  it("refuses a response that differs in ways params and fragments don't explain", () => {
    // As if the layout read a cookie, or a nonce changed per request.
    const other = { ...q7, document: q7.document.replace('<html lang="en">', '<html lang="de">') };
    expect(confirm(learned(q3), other)).toMatchObject({
      ok: false,
      reason: expect.stringContaining(
        "differs in ways the params and fragments don't explain",
      ) as string,
    });
    const extra = { ...q7, document: `${q7.document}<script>self.__next_f.push([1,"x"])</script>` };
    expect(confirm(learned(q3), extra)).toMatchObject({
      ok: false,
      reason: expect.stringContaining("structure") as string,
    });
  });
});

/** A document shaped like Next.js 16.4's, encoded as Next.js encodes its Flight scripts. */
function synthetic(p: string, extra = ""): Sample {
  const esc = (s: string) =>
    s.replace(
      /[&<>"']/g,
      (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#x27;" })[c] ?? c,
    );
  const jsonEsc = (s: string) =>
    s.replace(
      /[&><\u2028\u2029]/g,
      (c) =>
        ({
          "&": "\\u0026",
          ">": "\\u003e",
          "<": "\\u003c",
          "\u2028": "\\u2028",
          "\u2029": "\\u2029",
        })[c] ?? c,
    );
  const pathname = `/x/${p}`;
  const rows =
    `0:{"c":${JSON.stringify(pathname.split("/"))},"q":"","h":16,"k":${JSON.stringify(p)}}\n` +
    `9:["$","b",null,{"children":${JSON.stringify(p)}}]\n`;
  const document =
    `<html><head><title>${esc(p)} | site${extra}</title></head><body><b>${esc(p)}</b>` +
    `<script>self.__next_f.push(${jsonEsc(JSON.stringify([1, rows]))})</script></body></html>`;
  return {
    document,
    params: { p },
    pathname,
    search: "",
    fragments: [
      {
        name: "page",
        html: `<b>${esc(p)}</b>`,
        flight: JSON.stringify(["$", "b", null, { children: p }]),
      },
    ],
  };
}

describe("on documents shaped like Next.js's", () => {
  it("escapes params as each place writes them: HTML text, and JSON in Flight scripts", () => {
    const template = learned(synthetic("plain"));
    const result = confirm(template, synthetic(`a<b>&"c'\u2028`));
    if (!result.ok) throw new Error(result.reason);
    for (const p of [`x&amp;y`, `<script>`, "\u2029'\""]) {
      expect(fill(result.template, values(synthetic(p))).toString()).toBe(synthetic(p).document);
    }
  });

  it("tells a param from the same characters next to it", () => {
    // "1" is also in `"h":16`; with "16" as the other value, only backtracking tells them apart.
    const result = confirm(learned(synthetic("1")), synthetic("16"));
    if (!result.ok) throw new Error(result.reason);
    expect(fill(result.template, values(synthetic("7"))).toString()).toBe(synthetic("7").document);
  });

  it("refuses a response without the router state's URL", () => {
    const s = synthetic("a");
    const missing = { ...s, document: s.document.replace(/\\"c\\":\[[^\]]*\],/, "") };
    expect(learn(missing)).toMatchObject({
      ok: false,
      reason: `the router state's URL ("c") found 0 times in Next.js's response, not once`,
    });
  });

  it("refuses a confirmation that changes the wrapper around a param", () => {
    expect(confirm(learned(synthetic("a")), synthetic("b", " (new)"))).toMatchObject({ ok: false });
  });

  it("needs every fragment and param to fill", () => {
    const result = confirm(learned(synthetic("a")), synthetic("b"));
    if (!result.ok) throw new Error(result.reason);
    expect(() => fill(result.template, { ...values(synthetic("c")), fragments: {} })).toThrow(
      "no fragment page",
    );
    expect(() => fill(result.template, { ...values(synthetic("c")), params: {} })).toThrow();
  });
});

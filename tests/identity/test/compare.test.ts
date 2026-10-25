import { describe, expect, it } from "vitest";
import { compareDocuments, firstHtmlDifference, firstTreeDifference } from "../src/index.ts";

const flight = (rows: string) => `<script>self.__next_f.push([1,${JSON.stringify(rows)}])</script>`;
const BOOT = "<script>(self.__next_f=self.__next_f||[]).push([0])</script>";

/** A small Next.js-shaped document: shell, optional streamed boundary, Flight scripts. */
function page(options: { title?: string; streamed?: boolean; rows?: string }): string {
  const title = options.title ?? "Hello";
  const related = '<div class="py-8"><h2>Related</h2></div>';
  const body = options.streamed
    ? '<!--$?--><template id="B:0"></template><div class="animate-pulse"></div><!--/$-->'
    : `<!--$-->${related}<!--/$-->`;
  const rows =
    options.rows ??
    `0:["$","main",null,{"children":[["$","h1",null,{"children":"${title}"}],"$L1"]}]\n1:["$","h2",null,{"children":"Related"}]\n`;
  const tail = options.streamed
    ? `<div hidden id="S:0">${related}</div><script>$RB=[];$RC=function(a,b){};$RC("B:0","S:0")</script>`
    : "";
  return `<!DOCTYPE html><html><body><h1>${title}</h1>${body}${BOOT}${flight(rows)}${tail}</body></html>`;
}

describe("compareDocuments", () => {
  it("reports identical documents as identical", () => {
    const report = compareDocuments(page({}), page({}));
    expect(report.identical).toBe(true);
    expect(report.html.identical).toBe(true);
    expect(report.flight.identical).toBe(true);
  });

  it("treats a streamed boundary and the same content rendered complete as identical", () => {
    const report = compareDocuments(page({ streamed: true }), page({ streamed: false }));
    expect(report.identical).toBe(true);
    expect(report.streamed).toEqual({ expected: 1, actual: 0 });
  });

  it("treats the same Flight tree split into different rows as identical", () => {
    const inline =
      '0:["$","main",null,{"children":[["$","h1",null,{"children":"Hello"}],["$","h2",null,{"children":"Related"}]]}]\n';
    expect(compareDocuments(page({}), page({ rows: inline })).identical).toBe(true);
  });

  it("catches a one-byte difference in the HTML and says where it is", () => {
    const expected = page({ title: "Hello" });
    const actual = expected.replace("<h1>Hello</h1>", "<h1>Hellp</h1>");
    expect(actual).not.toBe(expected);
    const report = compareDocuments(expected, actual);
    expect(report.identical).toBe(false);
    expect(report.flight.identical).toBe(true);
    const offset = expected.indexOf("<h1>Hello</h1>") + "<h1>Hell".length;
    expect(report.html.difference).toMatchObject({ offset, line: 1, column: offset + 1 });
    expect(report.html.difference?.expected).toContain("Hello</h1>");
    expect(report.html.difference?.actual).toContain("Hellp</h1>");
  });

  it("catches a one-byte difference in the Flight tree and gives its path", () => {
    const rows = (title: string) =>
      `0:["$","main",null,{"children":[["$","h1",null,{"children":"${title}"}],"$L1"]}]\n1:["$","h2",null,{"children":"Related"}]\n`;
    const expected = page({ rows: rows("Hello") });
    const actual = page({ rows: rows("Hellp") });
    expect(actual).not.toBe(expected);
    const report = compareDocuments(expected, actual);
    expect(report.identical).toBe(false);
    expect(report.html.identical).toBe(true);
    expect(report.flight.difference).toEqual({
      path: "root.value.props.children.0.value.props.children",
      expected: "Hello",
      actual: "Hellp",
    });
  });
});

describe("firstHtmlDifference", () => {
  it("reports line and column on later lines, and a difference at the end", () => {
    expect(firstHtmlDifference("a\nbc\nd", "a\nbx\nd")).toMatchObject({
      offset: 3,
      line: 2,
      column: 2,
    });
    expect(firstHtmlDifference("abc", "abcd")).toMatchObject({
      offset: 3,
      expected: "abc",
      actual: "abcd",
    });
    expect(firstHtmlDifference("same", "same")).toBeUndefined();
  });
});

describe("firstTreeDifference", () => {
  it.each([
    ["a changed value", { a: 1 }, { a: 2 }, "a"],
    ["a missing key", { a: 1, b: 2 }, { a: 1 }, "b"],
    ["an extra array item", [1, 2], [1, 2, 3], "2"],
    ["an array vs an object", { a: [1] }, { a: { 0: 1 } }, "a"],
    ["null vs an object", { a: null }, { a: {} }, "a"],
    ["a different type", { a: "1" }, { a: 1 }, "a"],
    ["the same keys in a different order", { a: 1, b: 2 }, { b: 2, a: 1 }, "(root) (key order)"],
  ])("finds %s", (_name, expected, actual, path) => {
    expect(firstTreeDifference(expected, actual, [])?.path).toBe(path);
  });

  it("treats NaN and -0 by identity, not by ==", () => {
    expect(firstTreeDifference(Number.NaN, Number.NaN, [])).toBeUndefined();
    expect(firstTreeDifference(0, -0, [])?.path).toBe("(root)");
  });
});

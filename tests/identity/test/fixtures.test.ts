import { mkdtempSync, mkdirSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { describe, expect, it } from "vitest";
import { listFixtures, loadFixture, parseExpectations } from "../src/index.ts";

describe("parseExpectations", () => {
  it("accepts URLs and an optional streams flag", () => {
    expect(parseExpectations({ urls: ["/", "/a?b=1"] }, "x")).toEqual({
      urls: ["/", "/a?b=1"],
      streams: false,
    });
    expect(parseExpectations({ urls: ["/"], streams: true }, "x").streams).toBe(true);
  });

  it.each([
    ["a non-object", [], "must be an object"],
    ["null", null, "must be an object"],
    ["missing URLs", {}, '"urls" must be a non-empty array'],
    ["no URLs", { urls: [] }, '"urls" must be a non-empty array'],
    ["a URL that isn't a path", { urls: ["http://x/"] }, 'starting with "/"'],
    ["a non-string URL", { urls: [1] }, 'starting with "/"'],
    ["a non-boolean streams flag", { urls: ["/"], streams: "yes" }, '"streams" must be a boolean'],
    ["unknown keys", { urls: ["/"], url: "/" }, "unknown keys: url"],
  ])("rejects %s", (_name, json, message) => {
    expect(() => parseExpectations(json, "fixtures/x/expect.json")).toThrow(message);
  });
});

describe("loadFixture and listFixtures", () => {
  const root = mkdtempSync(path.join(tmpdir(), "tsx2rs-fixtures-"));
  const add = (name: string, expect: string) => {
    mkdirSync(path.join(root, name));
    writeFileSync(path.join(root, name, "expect.json"), expect);
  };
  add("b-second", '{"urls":["/b"]}');
  add("a-first", '{"urls":["/"],"streams":true}');
  writeFileSync(path.join(root, "README.md"), "not a fixture");

  it("lists fixture folders sorted by name, ignoring files", () => {
    expect(listFixtures(root).map((f) => [f.name, f.urls, f.streams])).toEqual([
      ["a-first", ["/"], true],
      ["b-second", ["/b"], false],
    ]);
  });

  it("reports which file is broken", () => {
    add("broken", "{not json");
    expect(() => loadFixture(path.join(root, "broken"))).toThrow(
      path.join(root, "broken", "expect.json"),
    );
  });
});

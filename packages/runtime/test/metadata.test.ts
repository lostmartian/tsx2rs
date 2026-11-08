import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { fill, fragmentNames, learn } from "../src/capture.js";
import { MetadataCache, findMetadata } from "../src/metadata.js";

/** Real Next.js 16.4.5 documents of Vercel Commerce's CMS pages (see test/next/README.md). */
const doc = (name: string) =>
  readFileSync(join(import.meta.dirname, "next", `commerce-${name}.html`), "utf8");

describe("findMetadata", () => {
  it("finds a page's metadata in Flight and in <head>", () => {
    const found = findMetadata(doc("about"));
    if (typeof found !== "object") throw new Error(String(found));
    expect(found.name).toBe("metadata");
    // The row the head's `Next.Metadata` boundary refers to.
    expect(found.flight).toMatch(/^\[\["\$","title","0",\{"children":"about \| Acme Store"\}\],/);
    // One tag per element, in order, from the title to the icon.
    expect(found.html).toMatch(/^<title>about \| Acme Store<\/title><meta name="description"/);
    expect(found.html).toMatch(
      /<link rel="icon" href="[^"]*" sizes="48x48" type="image\/x-icon"\/>$/,
    );
    // Every host element has its tag; Next.js's `IconMark`, a client element, has none.
    const elements = JSON.parse(found.flight) as [string, string][];
    const hosts = elements.filter(([, type]) => !type.startsWith("$"));
    expect(elements.length - hosts.length).toBe(1);
    expect(found.html.match(/<(title|meta|link)[ >]/g)).toHaveLength(hosts.length);
  });

  it("has nothing per request for a page without a metadata boundary, or with no elements", () => {
    expect(findMetadata("<html><head></head><body></body></html>")).toBeUndefined();
    const empty = `<head></head><script>self.__next_f.push(${JSON.stringify([1, '0:{"x":{"name":"Next.Metadata","children":"$L5"}}\n5:[]\n'])})</script>`;
    expect(findMetadata(empty)).toBeUndefined();
  });

  it("refuses metadata it can't find exactly once", () => {
    const script = (rows: string) =>
      `<script>self.__next_f.push(${JSON.stringify([1, rows])})</script>`;
    const boundary = '0:{"x":{"name":"Next.Metadata","children":"$L5"}}\n';
    expect(findMetadata(`<head></head>${script(boundary)}`)).toBe(
      "the metadata row is in Next.js's response 0 times, not once",
    );
    expect(findMetadata(`<head></head>${script(boundary + boundary)}`)).toBe(
      "the metadata boundary is in Next.js's response more than once",
    );
    expect(findMetadata(`<head></head>${script(`${boundary}5:{"a":1}\n`)}`)).toBe(
      "the metadata row isn't a list of elements",
    );
    expect(findMetadata(`<head></head>${script(`${boundary}5:[1]\n`)}`)).toBe(
      "the metadata row has something other than elements",
    );
    const title = `${boundary}5:[["$","title","0",{"children":"t"}]]\n`;
    // In Flight, but not in the HTML, or there twice.
    expect(findMetadata(`<head></head>${script(title)}`)).toBe(
      "the metadata's tags aren't in Next.js's <head> exactly once",
    );
    expect(findMetadata(`<title>t</title><title>t</title>${script(title)}`)).toBe(
      "the metadata's tags aren't in Next.js's <head> exactly once",
    );
    expect(findMetadata(`<title>t</title>${script(title)}`)).toMatchObject({
      html: "<title>t</title>",
    });
  });
});

describe("metadata in the captured wrapper", () => {
  it("becomes a fragment the template is filled with per request", () => {
    const about = findMetadata(doc("about"));
    const terms = findMetadata(doc("terms"));
    if (typeof about !== "object" || typeof terms !== "object") throw new Error("no metadata");
    const learned = learn({
      document: doc("about"),
      params: {},
      pathname: "/about",
      search: "",
      fragments: [about],
    });
    if (!learned.ok) throw new Error(learned.reason);
    expect(fragmentNames(learned.template)).toEqual(new Set(["metadata"]));
    const values = { params: {}, pathname: "/about", search: "", fragments: { metadata: about } };
    expect(fill(learned.template, values)).toBe(doc("about"));
    // Another request's metadata goes in the same places, in HTML and in Flight.
    const other = fill(learned.template, { ...values, fragments: { metadata: terms } });
    expect(other).toContain(terms.html);
    expect(other).not.toContain(about.html);
  });
});

describe("MetadataCache", () => {
  it("remembers each request's metadata, and pages without any", () => {
    const cache = new MetadataCache();
    const metadata = { name: "metadata", html: "<title>t</title>", flight: "[]" };
    cache.set("/a", metadata);
    cache.set("/b", null);
    expect(cache.get("/a")).toBe(metadata);
    expect(cache.get("/b")).toBeNull();
    expect(cache.get("/c")).toBeUndefined();
  });
});

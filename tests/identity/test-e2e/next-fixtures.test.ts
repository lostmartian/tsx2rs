/**
 * The harness against real Next.js output: every fixture app is built and served by two
 * `next start` processes. Next.js must be identical to itself across requests (streaming timing
 * varies, so this proves the harness has no false alarms), and one-byte changes to real output
 * must be caught.
 */
import path from "node:path";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import {
  buildNextApp,
  compareDocuments,
  listFixtures,
  normalizeDocument,
  startNextApp,
  type IdentityReport,
  type RunningServer,
} from "../src/index.ts";

const FIXTURES = path.resolve(import.meta.dirname, "../../fixtures");
const REQUESTS_PER_URL = 5;

const get = async (server: RunningServer, url: string) => {
  const response = await fetch(server.origin + url);
  expect(response.status, `${url} status`).toBe(200);
  return response.text();
};

const explain = (report: IdentityReport) => JSON.stringify(report, null, 2);

describe.each(listFixtures(FIXTURES))("fixture $name", (fixture) => {
  let a: RunningServer;
  let b: RunningServer;

  beforeAll(async () => {
    await buildNextApp(fixture.dir);
    [a, b] = await Promise.all([startNextApp(fixture.dir), startNextApp(fixture.dir)]);
  });

  afterAll(async () => {
    await Promise.all([a.stop(), b.stop()]);
  });

  it("is identical to itself across servers and requests", async () => {
    let streamed = 0;
    for (const url of fixture.urls) {
      for (let i = 0; i < REQUESTS_PER_URL; i++) {
        const [expected, actual] = await Promise.all([get(a, url), get(b, url)]);
        const report = compareDocuments(expected, actual);
        expect(report.identical, explain(report)).toBe(true);
        streamed += report.streamed.expected + report.streamed.actual;
      }
    }
    if (fixture.streams) expect(streamed, "boundaries streamed").toBeGreaterThan(0);
    else expect(streamed, "boundaries streamed").toBe(0);
  });

  it("catches a one-byte change in the page's text", async () => {
    const [url = "/"] = fixture.urls;
    const expected = await get(a, url);
    // Flip one bit of the first letter of the <h1> text (every fixture has one). The payload
    // escapes "<" as \u003c, so the first "<h1>" in the raw document is the element itself.
    const at = expected.indexOf("<h1>") + "<h1>".length;
    expect(at, "fixture has an <h1>").toBeGreaterThan(3);
    const flipped = String.fromCharCode(expected.charCodeAt(at) ^ 1);
    const actual = expected.slice(0, at) + flipped + expected.slice(at + 1);
    const report = compareDocuments(expected, actual);
    expect(report.identical).toBe(false);
    expect(report.flight.identical).toBe(true);
    const { html } = normalizeDocument(expected);
    expect(report.html.difference?.offset).toBe(html.indexOf("<h1>") + "<h1>".length);
  });
});

describe("fixture suspense-streamed", () => {
  it("catches a one-byte change in the Flight payload", async () => {
    const fixture = listFixtures(FIXTURES).find((f) => f.name === "suspense-streamed");
    expect(fixture).toBeDefined();
    if (fixture === undefined) return;
    const server = await startNextApp(fixture.dir);
    try {
      const expected = await get(server, "/");
      // In the payload the heading is the escaped JSON string \"Product\".
      const marker = String.raw`\"Product\"`;
      expect(expected.split(marker).length - 1, "one heading in the payload").toBe(1);
      const actual = expected.replace(marker, String.raw`\"Producu\"`);
      const report = compareDocuments(expected, actual);
      expect(report.identical).toBe(false);
      expect(report.html.identical).toBe(true);
      expect(report.flight.difference).toMatchObject({ expected: "Product", actual: "Producu" });
    } finally {
      await server.stop();
    }
  });
});

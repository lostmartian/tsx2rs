/**
 * Fixture apps in `tests/fixtures/<area>-<feature>/`: a minimal Next.js app per construct, with an
 * `expect.json` saying which URLs to compare (docs/engineering/testing.md#fixtures).
 */
import { readdirSync, readFileSync } from "node:fs";
import path from "node:path";

export interface Fixture {
  /** Folder name, e.g. `suspense-streamed`. */
  readonly name: string;
  readonly dir: string;
  /** Paths to request, each starting with `/`. */
  readonly urls: readonly string[];
  /** Whether Next.js streams at least one boundary for these URLs (checked by the tests). */
  readonly streams: boolean;
}

/** Validates the contents of an `expect.json` file. */
export function parseExpectations(
  json: unknown,
  source: string,
): Pick<Fixture, "urls" | "streams"> {
  const fail = (why: string): never => {
    throw new Error(`${source}: ${why}`);
  };
  if (json === null || typeof json !== "object" || Array.isArray(json)) fail("must be an object");
  const { urls, streams, ...rest } = json as Record<string, unknown>;
  const unknownKeys = Object.keys(rest);
  if (unknownKeys.length > 0) fail(`unknown keys: ${unknownKeys.join(", ")}`);
  if (!Array.isArray(urls) || urls.length === 0) fail('"urls" must be a non-empty array');
  for (const url of urls as unknown[]) {
    if (typeof url !== "string" || !url.startsWith("/")) {
      fail(`every URL must be a path starting with "/" (got ${JSON.stringify(url)})`);
    }
  }
  if (streams !== undefined && typeof streams !== "boolean") fail('"streams" must be a boolean');
  return { urls: urls as string[], streams: streams === true };
}

export function loadFixture(dir: string): Fixture {
  const source = path.join(dir, "expect.json");
  let json: unknown;
  try {
    json = JSON.parse(readFileSync(source, "utf8"));
  } catch (e) {
    throw new Error(`${source}: ${e instanceof Error ? e.message : String(e)}`, { cause: e });
  }
  return { name: path.basename(dir), dir, ...parseExpectations(json, source) };
}

/** Every fixture under `root`, sorted by name. */
export function listFixtures(root: string): Fixture[] {
  return readdirSync(root, { withFileTypes: true })
    .filter((entry) => entry.isDirectory())
    .map((entry) => entry.name)
    .sort()
    .map((name) => loadFixture(path.join(root, name)));
}

import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { afterAll, describe, expect, it } from "vitest";
import { compileApp, programFormatVersion } from "../src/index.js";

const root = mkdtempSync(join(tmpdir(), "tsx2rs-compile-app-"));
afterAll(() => {
  rmSync(root, { recursive: true, force: true });
});

function write(path: string, source: string) {
  mkdirSync(dirname(join(root, path)), { recursive: true });
  writeFileSync(join(root, path), source);
}

write("components/price.tsx", "export function Price({ amount }) { return <p>{amount}</p>; }\n");
write(
  "app/p/[n]/page.tsx",
  'import { Price } from "../../../components/price";\n' +
    "export default async function Page({ params }) {\n" +
    "  const { n } = await params;\n" +
    "  return <main><Price amount={n} /></main>;\n" +
    "}\n",
);
write("app/broken/page.tsx", "export default function Page() { return <div>{...xs}</div>; }\n");

describe("compileApp", () => {
  const app = compileApp(root, ["app/p/[n]/page.tsx", "app/broken/page.tsx"]);

  it("compiles a route to a program, an encoder and its data functions", () => {
    const [route] = app.routes;
    expect(route?.page).toBe("app/p/[n]/page.tsx");
    expect(route?.compiled).toBe(true);
    expect(route?.reasons).toEqual([]);
    // The program's binary format: an 8-byte magic, then its version (u16 LE).
    expect(route?.program?.subarray(0, 8).toString("latin1")).toBe("TSX2RSP\0");
    expect(route?.program?.readUInt16LE(8)).toBe(programFormatVersion());
    expect(route?.encoder).toContain("export function encode(record, data, islands, boundaries)");
    expect(route?.entry).toEqual({ module: "app/p/[n]/page.tsx", name: "__tsx2rs_data_Page" });
    expect(route?.callees).toEqual([
      { module: "components/price.tsx", name: "__tsx2rs_data_Price" },
    ]);
  });

  it("gives each server module the code the loader appends, registration included", () => {
    expect(Object.keys(app.modules).sort()).toEqual(["app/p/[n]/page.tsx", "components/price.tsx"]);
    expect(app.modules["components/price.tsx"]).toContain(
      'globalThis[Symbol.for("tsx2rs.data")] ??= new Map()).set("components/price.tsx", { __tsx2rs_data_Price });',
    );
  });

  it("says why a route falls back", () => {
    const broken = app.routes[1];
    expect(broken?.compiled).toBe(false);
    expect(broken?.program).toBeUndefined();
    expect(broken?.reasons.join("\n")).toContain("TSX2RS_E0005");
  });
});

import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { programFormatVersion, version } from "../src/index.js";

const cargoToml = readFileSync(new URL("../../../Cargo.toml", import.meta.url), "utf8");

describe("@tsx2rs/native", () => {
  it("loads the binary built from this workspace", () => {
    const workspaceVersion = /^version = "(.+)"$/m.exec(cargoToml)?.[1];
    expect(workspaceVersion).toBeDefined();
    expect(version()).toBe(workspaceVersion);
  });

  it("reports the template-program format version as a 16-bit integer", () => {
    const format = programFormatVersion();
    expect(Number.isInteger(format)).toBe(true);
    expect(format).toBeGreaterThanOrEqual(0);
    expect(format).toBeLessThanOrEqual(0xffff);
  });
});

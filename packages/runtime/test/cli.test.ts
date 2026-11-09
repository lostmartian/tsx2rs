import { availableParallelism } from "node:os";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";
import { parse } from "../src/cli.js";
import { VERIFY_EVERY, VERIFY_INTERVAL } from "../src/server.js";

describe("tsx2rs start", () => {
  it("defaults as next start does", () => {
    expect(parse([], {})).toEqual({
      dir: resolve("."),
      port: 3000,
      host: "0.0.0.0",
      workers: 1,
      verifyEvery: VERIFY_EVERY,
      verifyInterval: VERIFY_INTERVAL,
    });
    expect(parse([], { PORT: "8080" }).port).toBe(8080);
  });

  it("takes a folder, a port, a host, workers and the re-check rate", () => {
    expect(
      parse(
        [
          "app",
          "-p",
          "4000",
          "--host",
          "127.0.0.1",
          "--workers",
          "3",
          "--verify-every",
          "0",
          "--verify-interval",
          "250",
        ],
        { PORT: "8080" },
      ),
    ).toEqual({
      dir: resolve("app"),
      port: 4000,
      host: "127.0.0.1",
      workers: 3,
      verifyEvery: 0,
      verifyInterval: 250,
    });
    expect(parse(["--workers", "auto"], {}).workers).toBe(availableParallelism());
  });

  it("refuses what it doesn't understand", () => {
    expect(() => parse(["a", "b"], {})).toThrow("usage: tsx2rs start");
    expect(() => parse(["--workers", "0"], {})).toThrow("--workers must be an integer ≥ 1");
    expect(() => parse(["--port", "x"], {})).toThrow("--port must be an integer ≥ 0");
    expect(() => parse(["--verify-interval", "1.5"], {})).toThrow(
      "--verify-interval must be an integer ≥ 0",
    );
    expect(() => parse(["--nope"], {})).toThrow();
  });
});

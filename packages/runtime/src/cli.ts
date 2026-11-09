#!/usr/bin/env node
/**
 * `tsx2rs start`: serves a Next.js app built with `@tsx2rs/adapter` (ADR-0028), as `next start`
 * does, with its compiled routes served by tsx2rs and everything else by Next.js's own handler.
 *
 *     tsx2rs start [dir] [--port 3000] [--host 0.0.0.0] [--workers 1|auto] [--verify-every 100]
 *                  [--verify-interval 1000]
 *
 * With several workers, each is a whole server sharing the port (`node:cluster`), learning and
 * serving on its own; one that dies is replaced.
 */
import cluster from "node:cluster";
import { realpathSync } from "node:fs";
import { availableParallelism } from "node:os";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";
import { VERIFY_EVERY, VERIFY_INTERVAL, start } from "./server.ts";

/** What `tsx2rs start` was asked for. */
export interface StartOptions {
  dir: string;
  port: number;
  host: string;
  workers: number;
  verifyEvery: number;
  verifyInterval: number;
}

const USAGE = `usage: tsx2rs start [dir] [--port <port>] [--host <host>] [--workers <n|auto>] [--verify-every <n>] [--verify-interval <ms>]`;

/**
 * The options in `argv` (after `start`), with `next start`'s defaults: the current folder, port
 * `PORT` or 3000, every interface. Throws with the usage on anything else.
 */
export function parse(argv: string[], env: Record<string, string | undefined>): StartOptions {
  const { values, positionals } = parseArgs({
    args: argv,
    allowPositionals: true,
    options: {
      port: { type: "string", short: "p" },
      host: { type: "string", short: "H" },
      workers: { type: "string", short: "w" },
      "verify-every": { type: "string" },
      "verify-interval": { type: "string" },
    },
  });
  if (positionals.length > 1) throw new Error(USAGE);
  const number = (name: string, raw: string | undefined, fallback: number, min: number) => {
    if (raw === undefined) return fallback;
    const n = Number(raw);
    if (!Number.isInteger(n) || n < min)
      throw new Error(`--${name} must be an integer ≥ ${String(min)}\n${USAGE}`);
    return n;
  };
  return {
    dir: resolve(positionals[0] ?? "."),
    port: number("port", values.port ?? env["PORT"], 3000, 0),
    host: values.host ?? "0.0.0.0",
    workers:
      values.workers === "auto" ? availableParallelism() : number("workers", values.workers, 1, 1),
    verifyEvery: number("verify-every", values["verify-every"], VERIFY_EVERY, 0),
    verifyInterval: number("verify-interval", values["verify-interval"], VERIFY_INTERVAL, 0),
  };
}

/** Starts one server, or a primary that keeps `workers` servers running. */
export async function run(options: StartOptions): Promise<void> {
  if (options.workers > 1 && cluster.isPrimary) {
    for (let i = 0; i < options.workers; i++) cluster.fork();
    cluster.on("exit", (worker, code, signal) => {
      console.error(
        `tsx2rs: worker ${String(worker.process.pid)} exited (code ${String(code)}, signal ${signal}); starting another`,
      );
      cluster.fork();
    });
    console.log(
      `tsx2rs: ${String(options.workers)} workers on http://${options.host}:${String(options.port)}`,
    );
    return;
  }
  const server = await start({
    dir: options.dir,
    port: options.port,
    host: options.host,
    verifyEvery: options.verifyEvery,
    verifyInterval: options.verifyInterval,
  });
  if (options.workers === 1) {
    console.log(`tsx2rs: listening on http://${options.host}:${String(options.port)}`);
  }
  for (const signal of ["SIGINT", "SIGTERM"] as const) {
    process.once(signal, () => {
      server.close(() => process.exit(0));
    });
  }
}

async function main(argv: string[]): Promise<void> {
  const [command, ...rest] = argv;
  if (command !== "start") {
    console.error(USAGE);
    process.exit(2);
  }
  try {
    await run(parse(rest, process.env));
  } catch (error) {
    console.error(error instanceof Error ? error.message : String(error));
    process.exit(2);
  }
}

// Run as a command (the bin links here), not imported.
const invoked = process.argv[1];
if (invoked !== undefined && realpathSync(invoked) === fileURLToPath(import.meta.url)) {
  await main(process.argv.slice(2));
}

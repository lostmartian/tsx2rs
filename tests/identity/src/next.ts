/**
 * Builds and serves a Next.js app with its own `next` binary, as `next build` / `next start`.
 * Exercised by the fixture tests (`pnpm test:identity`), not by the unit tests.
 */
import { spawn, type ChildProcess } from "node:child_process";
import { createRequire } from "node:module";
import { createServer } from "node:net";
import path from "node:path";

export interface RunningServer {
  /** e.g. `http://127.0.0.1:41234` */
  readonly origin: string;
  /** Stops the server and waits for it to exit. */
  stop(): Promise<void>;
}

const ENV = { ...process.env, NEXT_TELEMETRY_DISABLED: "1", NODE_ENV: "production" };

/** The app's own `next` CLI entry point (so each app uses the Next.js version it depends on). */
function nextBin(appDir: string): string {
  return createRequire(path.join(appDir, "package.json")).resolve("next/dist/bin/next");
}

/** Runs `next build` in `appDir`. Rejects with the build output if it fails. */
export async function buildNextApp(appDir: string): Promise<void> {
  const child = spawn(process.execPath, [nextBin(appDir), "build"], { cwd: appDir, env: ENV });
  const output = collect(child);
  const code = await exitCode(child);
  if (code !== 0)
    throw new Error(`next build failed in ${appDir} (exit ${String(code)}):\n${output()}`);
}

/** Starts `next start` for an already built app on a free port, once it answers HTTP. */
export async function startNextApp(
  appDir: string,
  { timeoutMs = 30_000 }: { timeoutMs?: number } = {},
): Promise<RunningServer> {
  const port = await freePort();
  const origin = `http://127.0.0.1:${String(port)}`;
  const child = spawn(
    process.execPath,
    [nextBin(appDir), "start", "--hostname", "127.0.0.1", "--port", String(port)],
    { cwd: appDir, env: ENV },
  );
  const output = collect(child);
  const exited = exitCode(child);
  const stop = async () => {
    if (child.exitCode === null && child.signalCode === null) child.kill("SIGTERM");
    await exited;
  };

  const deadline = Date.now() + timeoutMs;
  for (;;) {
    if (child.exitCode !== null) {
      throw new Error(`next start exited early (${String(child.exitCode)}):\n${output()}`);
    }
    try {
      await fetch(origin, { signal: AbortSignal.timeout(2_000) });
      return { origin, stop };
    } catch {
      if (Date.now() > deadline) {
        await stop();
        throw new Error(
          `next start did not answer on ${origin} within ${String(timeoutMs)} ms:\n${output()}`,
        );
      }
      await new Promise((resolve) => setTimeout(resolve, 100));
    }
  }
}

/** A TCP port that was free a moment ago. */
async function freePort(): Promise<number> {
  return new Promise((resolve, reject) => {
    const server = createServer();
    server.unref();
    server.on("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const address = server.address();
      const port = typeof address === "object" && address !== null ? address.port : 0;
      server.close(() => {
        resolve(port);
      });
    });
  });
}

function collect(child: ChildProcess): () => string {
  const chunks: string[] = [];
  child.stdout?.on("data", (d: Buffer) => chunks.push(d.toString()));
  child.stderr?.on("data", (d: Buffer) => chunks.push(d.toString()));
  return () => chunks.join("");
}

function exitCode(child: ChildProcess): Promise<number | null> {
  return new Promise((resolve, reject) => {
    child.on("error", reject);
    child.on("exit", (code) => {
      resolve(code);
    });
  });
}

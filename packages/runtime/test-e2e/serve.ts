/** Starts the tsx2rs server for the app in the working directory on `PORT`. */
import { start } from "../src/server.ts";

const port = Number(process.env["PORT"]);
await start({ dir: process.cwd(), port });
console.log(`listening on ${String(port)}`);

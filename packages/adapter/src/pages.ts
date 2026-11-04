/**
 * An app's page files, and how Next.js names them (`sourcePage` in the adapter's outputs).
 */
import { existsSync, readdirSync } from "node:fs";
import { join } from "node:path";

/** Page files: `page.tsx`, `page.jsx`, `page.ts` or `page.js`. */
const PAGE = /^page\.(tsx|jsx|ts|js)$/;

/** The app directory, relative to the project: `src/app` if it exists, else `app`. */
export function appDir(projectDir: string): string | undefined {
  for (const dir of ["src/app", "app"]) {
    if (existsSync(join(projectDir, dir))) return dir;
  }
  return undefined;
}

/** Every page file of the app, relative to the project with `/` separators, sorted. */
export function findPages(projectDir: string): string[] {
  const app = appDir(projectDir);
  if (app === undefined) return [];
  const pages: string[] = [];
  const walk = (dir: string) => {
    for (const entry of readdirSync(join(projectDir, dir), { withFileTypes: true })) {
      const path = `${dir}/${entry.name}`;
      if (entry.isDirectory()) {
        if (entry.name !== "node_modules") walk(path);
      } else if (PAGE.test(entry.name)) {
        pages.push(path);
      }
    }
  };
  walk(app);
  return pages.sort();
}

/** The page file's name in Next.js's build outputs: `app/p/[n]/page.tsx` → `/p/[n]/page`. */
export function sourcePage(page: string): string {
  return page.replace(/^(src\/)?app/, "").replace(/\.(tsx|jsx|ts|js)$/, "");
}

# Compiler diagnostics

When the compiler can't guarantee output identical to Next.js for a route, it reports a diagnostic, and the route falls back to Next.js ([ADR-0002](adr/0002-identical-output-to-nextjs.md)). Each diagnostic has a stable code, a message and a source location.

| Code | Name | Meaning | What to do |
|---|---|---|---|
| `TSX2RS_E0001` | Parse error | A file the route's server components import doesn't parse as TypeScript or JavaScript with JSX. | Fix the syntax error (`next build` reports it too). |
| `TSX2RS_E0002` | Unresolved import | A relative or aliased import (`./x`, `@/x`, a `tsconfig` `baseUrl` path) doesn't resolve to a file. Bare package names (`react`, `next/link`) are never reported here: they are packages. | Check the path and the `tsconfig.json` `baseUrl` / `paths`. |
| `TSX2RS_E0003` | Dynamic import | A server module uses `import()`, so its dependencies can't be known at build time. Dynamic imports inside client components are fine: their imports aren't part of the server render. | Use a static `import`, or accept the fallback for this route. |

Codes are never reused or renumbered. A new kind of problem gets a new code.

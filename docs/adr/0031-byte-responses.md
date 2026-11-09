# ADR-0031: Compiled responses are assembled and written as bytes

- **Status:** Accepted
- **Date:** 2026-11-09

## Context

A compiled response used to make several round trips between bytes and strings:

1. The engine wrote UTF-8 bytes. The napi binding checked them and made JavaScript strings of the
   HTML and every Flight row.
2. The runtime put the rows' ids in with a regular expression (`withIds`), then escaped each row
   for its `<script>` with `JSON.stringify` and a second regular expression (Next.js's
   `htmlEscapeJsonString`).
3. `fill` joined the template's text and these strings into one string of the whole document,
   about 100 KB on Commerce's product page.
4. `res.end(string)` encoded it back to UTF-8.

After ADR-0030, profiling `tsx2rs start` on that page put the encoding in step 4 (`utf8Write`) at
about 11% of the CPU, and the escaping at about 3%.

## Decision

- **The napi `Program.render` returns `Buffer`s** for the HTML, the segments and the Flight rows,
  handed over without a copy. They are still checked to be UTF-8. The head and hints stay strings:
  they are empty on every page tsx2rs serves.
- **The engine escapes the rows for their scripts and puts their ids in, in one pass.**
  - `tsx2rs_engine::flight_script` does it for one row.
  - The napi `flightScripts(rows, ids)` does it for all of a page's rows in one call.
  - It is the same escaping as `script_string` (now also one pass instead of two), with
    `"$L!<n>"` and `"$!<n>:` replaced as `withIds` does.
- **`fill` returns a `Buffer`.**
  - A template's fixed parts and units are encoded once and kept.
  - A fragment that carries `bytes` (its HTML, and its row escaped for the script) goes in as it
    is.
  - Anything else (learned metadata, tests) is encoded from its strings, and kept per fragment.
- **The response is written with one `res.end(buffer)`.**
- **Strings are made only when they are needed.** The renderer's fragments make their `html` and
  `flight` strings lazily, on first access: learning and re-checks read them, serving doesn't.
- **A re-check compares bytes** (Next.js's body encoded, against ours).

## Consequences

- The same bytes as before. A unit test fills a template from engine bytes and from strings, with
  characters both sides escape, and checks they match. Commerce's end-to-end identity tests pass.
  A property test checks the engine's escaping and id placement against the rules written out
  the long way.
- Assembling Commerce's product page (escaping, ids, joining, encoding) went from about 610–650 µs
  to 160–215 µs, in back-to-back runs on a throttled laptop. In the server profile, `utf8Write`
  and the JavaScript escaping are gone.
- Each `Buffer` handed across napi costs a fixed amount (about 2 µs). One buffer per page part is
  fine; a page with many rows could get them as one buffer and offsets later.
- `Fragment` gains an optional `bytes`. Code that builds fragments by hand keeps working, through
  the strings.

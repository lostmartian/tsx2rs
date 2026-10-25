# Fixture: jsx-basics

A page rendered on every request (`force-dynamic`), without streaming. It covers React's HTML output rules for:

- text next to expressions (`Items: {n} in stock: {m}`, separated by `<!-- -->`)
- escaping of `&`, `<`, `>`, `"` and `'` in text and attributes
- `className`, `data-*`, boolean attributes (`readOnly`, `checked`, `disabled`) and `style` objects
- lists with keys, conditionals, `null`/`false`/`""`/`0` children, and void elements (`<br>`, `<input>`)
- `dangerouslySetInnerHTML` in a JSON-LD `<script>`

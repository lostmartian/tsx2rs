import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { Input, node, primitive } from "@tsx2rs/runtime/encode";
import { afterAll, describe, expect, it } from "vitest";
import { compileApp, Program } from "../src/index.js";

const root = mkdtempSync(join(tmpdir(), "tsx2rs-program-"));
afterAll(() => {
  rmSync(root, { recursive: true, force: true });
});
mkdirSync(join(root, "app"), { recursive: true });
writeFileSync(
  join(root, "app/page.tsx"),
  'export default function Page({ x }) { return <p className="a">{x}<img src="/i.png" /></p>; }\n',
);

const [route] = compileApp(root, ["app/page.tsx"]).routes;
if (!route?.program) throw new Error("the page didn't compile");
const program = new Program(route.program);

/** `<img src="/i.png" />` as React builds it: an element value, which the runtime renders. */
const img = {
  $$typeof: Symbol.for("react.transitional.element"),
  type: "img",
  key: null,
  props: { src: "/i.png" },
};

/** The page's input: the root's wrap and key (ADR-0008), `{x}`, the `<img>`, the wrap again. */
function input(x: unknown): Buffer {
  const w = new Input();
  w.condition(false);
  primitive(w, null);
  node(w, x);
  node(w, img);
  w.condition(false);
  return Buffer.from(w.finish());
}

describe("Program", () => {
  it("renders a compiled program's HTML, Flight, head and hints", () => {
    const out = program.render(input("<hi>"));
    expect(out.html).toBe('<p class="a">&lt;hi&gt;<img src="/i.png"/></p>');
    // The Flight model, not yet escaped for a <script>.
    expect(out.flight).toBe(
      '["$","p",null,{"className":"a","children":["<hi>",["$","img",null,{"src":"/i.png"}]]}]',
    );
    // React DOM preloads the image in <head>, and React's Flight server hints it.
    expect(out.head).toBe('<link rel="preload" as="image" href="/i.png"/>');
    expect(out.hints).toBe(':HL["/i.png","image"]\n');
  });

  it("is reusable across renders", () => {
    expect(program.render(input(1)).html).toBe('<p class="a">1<img src="/i.png"/></p>');
    expect(program.render(input(2)).html).toBe('<p class="a">2<img src="/i.png"/></p>');
  });

  it("throws, never crashes, on a bad program or input", () => {
    expect(() => new Program(Buffer.from("not a program"))).toThrow("invalid program");
    expect(() => program.render(Buffer.from([0]))).toThrow("render failed");
  });
});

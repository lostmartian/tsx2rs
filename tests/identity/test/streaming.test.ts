import fc from "fast-check";
import { describe, expect, it } from "vitest";
import { IdentityError, resolveStreamedHtml } from "../src/index.ts";

/** React DOM 19.3.1 prefixes the first `$RC` call with its runtime; the body doesn't matter here. */
const RUNTIME = "$RB=[];$RV=function(a){a.length=0};\n$RC=function(a,b){};";
const rc = (b: string, s: string, runtime = "") => `<script>${runtime}$RC("${b}","${s}")</script>`;
const segment = (id: string, content: string) => `<div hidden id="${id}">${content}</div>`;
const pending = (id: string, fallback: string) =>
  `<!--$?--><template id="${id}"></template>${fallback}<!--/$-->`;

function codeOf(fn: () => unknown): string | undefined {
  try {
    fn();
  } catch (e) {
    if (e instanceof IdentityError) return e.code;
    throw e;
  }
  return undefined;
}

describe("resolveStreamedHtml", () => {
  it("leaves a document without streaming instructions unchanged", () => {
    const html = "<html><body><!--$--><p>Hi</p><!--/$--></body></html>";
    expect(resolveStreamedHtml(html)).toEqual({ html, applied: [] });
  });

  it("completes a boundary the way React's $RC does", () => {
    const html =
      `<main>${pending("B:0", "<p>Loading</p>")}</main>` +
      segment("S:0", "<p>Done</p>") +
      rc("B:0", "S:0", RUNTIME);
    expect(resolveStreamedHtml(html)).toEqual({
      html: "<main><!--$--><p>Done</p><!--/$--></main>",
      applied: [{ kind: "completeBoundary", args: ["B:0", "S:0"] }],
    });
  });

  it("removes a fallback that itself contains completed boundaries", () => {
    const html =
      pending("B:0", "<!--$--><i>inner</i><!--/$--><b>spinner</b>") +
      "<footer></footer>" +
      segment("S:0", "ok") +
      rc("B:0", "S:0");
    expect(resolveStreamedHtml(html).html).toBe("<!--$-->ok<!--/$--><footer></footer>");
  });

  it("resolves a boundary that arrives inside another streamed boundary", () => {
    const html =
      pending("B:0", "…") +
      segment("S:0", `<section>${pending("B:1", "…")}</section>`) +
      rc("B:0", "S:0", RUNTIME) +
      segment("S:1", "<p>deep</p>") +
      rc("B:1", "S:1");
    expect(resolveStreamedHtml(html).html).toBe(
      "<!--$--><section><!--$--><p>deep</p><!--/$--></section><!--/$-->",
    );
  });

  it("matches the segment's own closing tag, ignoring markup inside raw-text elements", () => {
    const content =
      '<div><div>a</div></div><script type="application/ld+json">{"x":"</div><div>"}</script>';
    const html = pending("B:0", "") + segment("S:0", content) + rc("B:0", "S:0");
    expect(resolveStreamedHtml(html).html).toBe(`<!--$-->${content}<!--/$-->`);
  });

  it("only removes the segment when the boundary's template is gone (React's behaviour)", () => {
    const html = "<p>shell</p>" + segment("S:0", "late") + rc("B:0", "S:0");
    expect(resolveStreamedHtml(html).html).toBe("<p>shell</p>");
  });

  it("does nothing for a boundary whose segment is missing (React's behaviour)", () => {
    const html = pending("B:0", "wait") + rc("B:0", "S:0");
    expect(resolveStreamedHtml(html).html).toBe(pending("B:0", "wait"));
  });

  it("completes a segment the way React's $RS does", () => {
    const html =
      '<ul><li>a</li><template id="P:1"></template></ul>' +
      segment("S:1", "<li>b</li>") +
      '<script>$RS=function(a,b){};$RS("S:1","P:1")</script>';
    expect(resolveStreamedHtml(html)).toEqual({
      html: "<ul><li>a</li><li>b</li></ul>",
      applied: [{ kind: "completeSegment", args: ["S:1", "P:1"] }],
    });
  });

  it("gives the same result whether a boundary streamed or rendered complete", () => {
    const content = '<div class="py-8"><h2>Related</h2></div>';
    const inline = `<main><!--$-->${content}<!--/$--></main><footer>f</footer>`;
    const streamed =
      `<main>${pending("B:0", '<div class="animate-pulse"></div>')}</main><footer>f</footer>` +
      segment("S:0", content) +
      rc("B:0", "S:0", RUNTIME);
    expect(resolveStreamedHtml(streamed).html).toBe(resolveStreamedHtml(inline).html);
  });

  it("leaves documents without instructions untouched (property)", () => {
    const fragment = fc.constantFrom(
      "<div>",
      "</div>",
      "<p class='x'>",
      "</p>",
      "<!--$-->",
      "<!--/$-->",
      "<script>var a=1</script>",
      "text",
      " & < > ",
    );
    fc.assert(
      fc.property(fc.array(fragment, { maxLength: 40 }), (parts) => {
        const html = parts.join("");
        expect(resolveStreamedHtml(html).html).toBe(html);
      }),
    );
  });

  describe("refuses what it doesn't model", () => {
    it.each([
      ["$RX (client-rendered error boundary)", '<script>$RX("B:0","digest")</script>'],
      ["$RX with only an id", '<script>$RX("B:0")</script>'],
      ["$RR (boundary with stylesheets)", '<script>$RR("B:0","S:0",[])</script>'],
      ["external-runtime instructions", '<template data-rsi="" data-sid="S:0"></template>'],
      [
        "a segment streamed in a non-div container",
        pending("B:0", "") + '<svg aria-hidden="true" id="S:0"></svg>' + rc("B:0", "S:0"),
      ],
    ])("%s", (_name, html) => {
      expect(codeOf(() => resolveStreamedHtml(html))).toBe("UNSUPPORTED_STREAM_INSTRUCTION");
    });
  });

  describe("rejects streams React wouldn't produce", () => {
    it.each([
      [
        "a boundary template without the pending comment",
        '<template id="B:0"></template>x<!--/$-->' + segment("S:0", "y") + rc("B:0", "S:0"),
      ],
      [
        "a boundary without its closing comment",
        '<!--$?--><template id="B:0"></template>x' + segment("S:0", "y") + rc("B:0", "S:0"),
      ],
      [
        "a boundary id on a non-template element",
        '<!--$?--><p id="B:0"></p><!--/$-->' + segment("S:0", "y") + rc("B:0", "S:0"),
      ],
      [
        "an instruction that isn't the script's last call",
        '<script>$RC("B:0","S:0");go()</script>',
      ],
      ["unparseable instruction arguments", "<script>$RC(B0,S0)</script>"],
      [
        "$RS with a missing segment",
        '<template id="P:1"></template><script>$RS("S:1","P:1")</script>',
      ],
      ["$RS with a missing placeholder", segment("S:1", "x") + '<script>$RS("S:1","P:1")</script>'],
      ["an instruction with the wrong number of ids", '<script>$RC("B:0")</script>'],
      ["an unterminated script", '<script>$RC("B:0","S:0")'],
      [
        "a segment that is never closed",
        pending("B:0", "") + '<div hidden id="S:0"><p>x</p>' + rc("B:0", "S:0"),
      ],
    ])("%s", (_name, html) => {
      expect(codeOf(() => resolveStreamedHtml(html))).toBe("MALFORMED_STREAM");
    });
  });
});

// Character references in JSX text and attribute strings, which the JSX transform decodes:
// named (`&copy;`), decimal and hex, next to holes, and ones it leaves as written.
export default function Page({ year, name }: { year: number; name: string }) {
  return (
    <footer title='&copy; &amp; "A" &#169;&#xA9;'>
      <p>
        &copy; {year} {name}. All&nbsp;rights &amp; more &mdash; &hellip;
      </p>
      <p>&#60;tag&#x3E; &notanentity; &amp &; &thetasym; &AElig;</p>
      <a href="/a?x=1&amp;y=2" data-label="&lt;b&gt;">
        &#128512;
      </a>
    </footer>
  );
}

export const cases = [
  { year: 2026, name: "Acme Inc" },
  { year: 0, name: "&copy;" },
];

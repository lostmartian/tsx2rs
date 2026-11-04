// Props React DOM writes by its own rules: className, booleans, numbers, style objects, URLs,
// data and aria attributes, and a spread whose later props override earlier ones. (`<img>` and
// `<link>` hoist into `<head>`; the engine's own differential tests cover those.)
export default function Page({
  cls,
  hidden,
  width,
  color,
  href,
  rest,
}: {
  cls: string | undefined;
  hidden: boolean;
  width: number;
  color: string | undefined;
  href: string;
  rest: Record<string, unknown>;
}) {
  return (
    <div className={cls} hidden={hidden} data-width={width} aria-label="box">
      <span style={{ color, marginTop: width }}>styled</span>
      <a href={href} target="_blank">
        link
      </a>
      <input type="text" readOnly={hidden} />
      <p id="fixed" {...rest} title="last">
        spread
      </p>
    </div>
  );
}

export const cases = [
  {
    cls: "a b",
    hidden: true,
    width: 10,
    color: "red",
    href: "/x?y=1&z=2",
    rest: { id: "over", "data-x": 1 },
  },
  {
    cls: undefined,
    hidden: false,
    width: 0,
    color: undefined,
    href: "javascript:alert(1)",
    rest: {},
  },
];

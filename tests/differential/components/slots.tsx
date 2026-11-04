// Slots: JSX children and JSX props a callee only places, compiled with the caller, including
// a linked component inside a slot and one placed in a branch.
import type { ReactNode } from "react";

function Card({
  title,
  children,
  footer,
}: {
  title: ReactNode;
  children: ReactNode;
  footer: ReactNode;
}) {
  return (
    <article>
      <header>{title}</header>
      {children}
      <footer>{footer}</footer>
    </article>
  );
}
function Toggle({ on, children }: { on: boolean; children: ReactNode }) {
  return <div>{on ? children : null}</div>;
}
function Name({ name }: { name: string }) {
  return <i>{name}</i>;
}
export default function Page({ title, name, on }: { title: string; name: string; on: boolean }) {
  return (
    <section>
      <Card title={<h2>{title}</h2>} footer="plain">
        <p key="k">first</p>
        text {name}
        <Name name={name} />
      </Card>
      <Card title={title} footer={<small>{name}</small>}>
        only
      </Card>
      <Toggle on={on}>
        <b>{name}</b>
      </Toggle>
    </section>
  );
}

export const cases = [
  { title: "T", name: "ada", on: true },
  { title: "", name: "<b>", on: false },
];

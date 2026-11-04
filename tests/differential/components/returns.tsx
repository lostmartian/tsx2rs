// Several returns: React renders whichever runs, `null` included.
export default function Page({ user, admin }: { user: string | null; admin: boolean }) {
  if (!user) return null;
  if (admin) {
    return <strong>admin {user}</strong>;
  }
  return <span>user {user}</span>;
}

export const cases = [
  { user: null, admin: false },
  { user: "ada", admin: true },
  { user: "bob", admin: false },
];

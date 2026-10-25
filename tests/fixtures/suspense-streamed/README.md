# Fixture: suspense-streamed

A dynamic page whose `<Suspense>` boundary streams: `Recommendations` waits 100 ms, so Next.js sends the shell first and the boundary's content later with React's `$RC` instruction. It also has a client component (`Counter`), so the Flight payload contains a client reference (`I` row) and the HTML contains its server-rendered output.

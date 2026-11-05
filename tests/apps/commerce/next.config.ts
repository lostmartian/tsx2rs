// tsx2rs test app: upstream used Next.js 15-canary experiments (ppr, useCache, inlineCss) that
// Next.js 16 renamed or removed; this runs as a normal dynamic app. Images are local test images.
export default {
  // tsx2rs: the one line milestone 1 allows (PHASE1-PLAN.md, acceptance criterion 1).
  adapterPath: import.meta.resolve("@tsx2rs/adapter"),
  images: {
    formats: ["image/avif", "image/webp"],
  },
};

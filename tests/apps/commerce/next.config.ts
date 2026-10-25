// tsx2rs test app: upstream used Next.js 15-canary experiments (ppr, useCache, inlineCss) that
// Next.js 16 renamed or removed; this runs as a normal dynamic app. Images are local test images.
export default {
  images: {
    formats: ["image/avif", "image/webp"],
  },
};

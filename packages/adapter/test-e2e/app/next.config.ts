import type { NextConfig } from "next";

// `TSX2RS_ADAPTER=off` builds without the adapter, to compare client bundles.
const config: NextConfig =
  process.env.TSX2RS_ADAPTER === "off"
    ? {}
    : { adapterPath: import.meta.resolve("@tsx2rs/adapter") };

export default config;

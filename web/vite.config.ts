/// <reference types="vitest/config" />
import { defineConfig, type Plugin } from "vite";
import react from "@vitejs/plugin-react";
import { fileURLToPath } from "node:url";

// In the highlighter worker, refractor decodes HTML entities in the few
// attributes Prism adds (a title on markup entities). The decoder's browser
// build needs `document`, which a worker does not have, and renderHast drops
// every attribute but the class anyway, so the worker gets a no-op.
const noEntityDecoding: Plugin = {
  name: "worker-no-entity-decoding",
  enforce: "pre",
  resolveId: (id) => (id === "parse-entities" ? "\0parse-entities" : null),
  load: (id) => (id === "\0parse-entities" ? "export const parseEntities = (value) => value;" : null),
};

// refractor builds its tokens with hastscript's `h`, whose property tables
// (property-information) the chat chunk bundles too; the worker gets a
// small `h` that does only what refractor asks (src/md/hast-h.ts).
const smallHast: Plugin = {
  name: "worker-small-hastscript",
  enforce: "pre",
  resolveId: (id) => (id === "hastscript" ? fileURLToPath(new URL("./src/md/hast-h.ts", import.meta.url)) : null),
};

// `npm run dev` proxies the API to a running `rizzy-harness serve`. Start it with
// `--allowed-host localhost:5173` so the dev origin passes the Origin check.
export default defineConfig({
  plugins: [react()],
  server: {
    proxy: {
      "/api": { target: "http://127.0.0.1:7777", ws: true },
    },
  },
  build: { outDir: "dist", emptyOutDir: true, sourcemap: false, manifest: true },
  // The highlighter worker loads each grammar as its own chunk, which needs
  // an ES module worker.
  worker: { format: "es", plugins: () => [noEntityDecoding, smallHast] },
  // theme.test.ts reads the stylesheet as text (`?raw`); vitest otherwise
  // turns every CSS import into an empty string.
  test: { css: { include: [/styles\.css/] } },
});

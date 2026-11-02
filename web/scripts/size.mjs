// Bundle budget (PLAN.md Phase 3): the new-tab home screen must stay small.
// Reads Vite's manifest; gzips every JS chunk; fails if over budget.
import { readFileSync } from "node:fs";
import { gzipSync } from "node:zlib";
import { join } from "node:path";

const INITIAL_KB = 110;
const TOTAL_KB = 300;
const dist = new URL("../dist/", import.meta.url).pathname;
const manifest = JSON.parse(readFileSync(join(dist, ".vite/manifest.json"), "utf8"));

const gz = (file) => gzipSync(readFileSync(join(dist, file))).length / 1024;
const entries = Object.values(manifest).filter((c) => c.isEntry);

// Initial = entry chunks plus their static imports (not dynamic ones).
const initial = new Set();
const walk = (key) => {
  const c = manifest[key];
  if (!c || initial.has(c.file)) return;
  initial.add(c.file);
  (c.imports ?? []).forEach(walk);
};
for (const [key, c] of Object.entries(manifest)) if (c.isEntry) walk(key);

const allJs = [...new Set(Object.values(manifest).map((c) => c.file))].filter((f) => f.endsWith(".js"));
const initialKb = [...initial].filter((f) => f.endsWith(".js")).reduce((a, f) => a + gz(f), 0);
const totalKb = allJs.reduce((a, f) => a + gz(f), 0);

console.log(`initial JS: ${initialKb.toFixed(1)} KB gzip (budget ${INITIAL_KB})`);
console.log(`total JS:   ${totalKb.toFixed(1)} KB gzip (budget ${TOTAL_KB}), ${allJs.length} chunks, ${entries.length} entry`);
if (initialKb > INITIAL_KB || totalKb > TOTAL_KB) {
  console.error("bundle budget exceeded");
  process.exit(1);
}

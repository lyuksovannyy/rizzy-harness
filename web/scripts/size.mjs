// Bundle budget (PLAN.md Phase 3): the new-tab home screen must stay small.
// Reads Vite's manifest; gzips every JS chunk; prints them; fails if over
// budget or if heavy code lands where it must not (see the checks below).
import { readdirSync, readFileSync } from "node:fs";
import { gzipSync } from "node:zlib";
import { join } from "node:path";

const INITIAL_KB = 110;
const TOTAL_KB = 300;
const dist = new URL("../dist/", import.meta.url).pathname;
const manifest = JSON.parse(readFileSync(join(dist, ".vite/manifest.json"), "utf8"));

const read = (file) => readFileSync(join(dist, file));
const gz = (file) => gzipSync(read(file)).length / 1024;
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

// Every emitted chunk counts. The manifest lists the main build only, not
// the highlighter worker or the grammar chunks it loads.
const allJs = readdirSync(join(dist, "assets"))
  .filter((f) => f.endsWith(".js"))
  .map((f) => `assets/${f}`);
const sizes = new Map(allJs.map((f) => [f, gz(f)]));
const initialKb = [...initial].filter((f) => f.endsWith(".js")).reduce((a, f) => a + sizes.get(f), 0);
const totalKb = allJs.reduce((a, f) => a + sizes.get(f), 0);

for (const f of [...allJs].sort((a, b) => sizes.get(b) - sizes.get(a))) {
  console.log(`  ${sizes.get(f).toFixed(1).padStart(6)} KB  ${initial.has(f) ? "initial " : "        "}${f}`);
}
console.log(`initial JS: ${initialKb.toFixed(1)} KB gzip (budget ${INITIAL_KB})`);
console.log(`total JS:   ${totalKb.toFixed(1)} KB gzip (budget ${TOTAL_KB}), ${allJs.length} chunks, ${entries.length} entry`);

// Strings that only the named library contains (they survive minification).
const failures = [];
const has = (file, re) => re.test(read(file).toString("utf8"));
for (const f of allJs) {
  // micromark-extension-math's HTML half imports KaTeX; it must tree-shake out.
  if (has(f, /KaTeX parse error/)) failures.push(`${f} contains KaTeX`);
  if (has(f, /mermaidAPI/)) failures.push(`${f} contains mermaid`);
}
// Math and highlighting load lazily, never with the home screen or a chat.
const eager = [...initial, ...allJs.filter((f) => /\/Conversation-/.test(f))];
for (const f of eager) {
  if (has(f, /temml/i)) failures.push(`${f} contains Temml`);
  if (has(f, /Unknown language|languages\.extend/)) failures.push(`${f} contains the highlighter`);
}
// A worker has no DOM: a browser build that needs one breaks highlighting.
for (const f of allJs.filter((f) => /\/highlight\.worker-/.test(f))) {
  if (has(f, /\b(document|window)\./)) failures.push(`${f} uses the DOM, which a worker does not have`);
}
for (const msg of failures) console.error(msg);
if (initialKb > INITIAL_KB || totalKb > TOTAL_KB) failures.push("bundle budget exceeded");
if (failures.length) {
  console.error(failures.at(-1));
  process.exit(1);
}

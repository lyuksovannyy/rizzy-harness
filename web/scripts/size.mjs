// Bundle budget (PLAN.md Phase 3 and "Rich messages"): the new-tab home
// screen must stay small, and heavy libraries must stay in the lazy chunks
// that need them. Reads Vite's manifest, prints every emitted file with its
// gzip size, and fails if over budget or if a library lands where it must
// not (see the markers below).
import { existsSync, readdirSync, readFileSync } from "node:fs";
import { gzipSync } from "node:zlib";
import { join } from "node:path";
import { createRequire } from "node:module";

const INITIAL_KB = 110;
// 300 before workspaces: the switcher, the Settings sections and the workspace
// panels brought the total to 304.9 KB, almost all of it in the lazy Settings
// chunk (the initial load stayed at 96.1 KB). 310, the ceiling the workspaces
// design allows, leaves room for its follow-up fixes without a raise each time.
// The Agents page (a lazy chunk of about 5 KB, with Settings → Agents removed)
// then took it to 309.9 KB on 2026-11-08.
// 321 for Projects (docs/projects.md), measured 320.2 KB the same day: 10.3 KB
// on top, 5.5 in its lazy page, 2.2 for the 20 icons the sidebar draws (in the
// first load, which stays under its own budget at 100.1) and the rest in the
// sidebar section, the shell and the chat header's picker.
//
// From the sidebar areas on (routines, media, artifacts, search), the budget
// is the last merged measurement plus each implementer's own growth, one line
// each, so parallel branches merge without touching the same line. Inside a
// phase a branch sets only its own line (its measured growth + at most 0.5,
// with the reason); at each merged phase BASE_KB is re-measured and the three
// lines go back to 0.

// Measured total of the last merged phase, with what it bought.
const BASE_KB = 348.8; // Phase A merged (measured 348.7x, rounded up): routines P1, storage + media M1, sidebar/search panel/account menu, routines and media pages (measured 2026-11-08; initial 103.7)

// I2: its own growth over BASE_KB + ≤ 0.5, with the reason on the line.
const I2_KB = 0.4; // Phase B: fold.ts artifact_staged / artifact_stage_failed items, all in the initial load (measured +0.2, 349.0 total / 103.9 initial, 2026-11-08) + 0.2

// I3: its own growth over BASE_KB + ≤ 0.5, with the reason on the line.
const I3_KB = 0;

// I4: its own growth over BASE_KB + ≤ 0.5, with the reason on the line.
const I4_KB = 0;

// Rounded to 0.01 first, so a float sum (0.1 + 0.2) cannot round a tenth up.
const TOTAL_KB = Math.ceil(Math.round((BASE_KB + I2_KB + I3_KB + I4_KB) * 100) / 10) / 10;
// The ceilings the sidebar areas were planned within; no budget line may
// take the bundle past them.
const PROGRAM_TOTAL_KB = 362;
// Raised from 103 once the Phase A pages merged (103.7 measured): the new
// sidebar, account button and routine-result rows are on the first screen,
// and the Phase C image attach joins the composer there. Still under the
// long-standing 110 KB first-load budget.
const PROGRAM_INITIAL_KB = 106;
const web = new URL("../", import.meta.url).pathname;
const dist = join(web, "dist");
const manifest = JSON.parse(readFileSync(join(dist, ".vite/manifest.json"), "utf8"));

const read = (file) => readFileSync(file.startsWith("/") ? file : join(dist, file));
const gz = (file) => gzipSync(read(file)).length / 1024;
const failures = [];

// Initial = entry chunks plus their static imports (not dynamic ones).
const initial = new Set();
const walk = (key) => {
  const c = manifest[key];
  if (!c || initial.has(c.file)) return;
  initial.add(c.file);
  (c.imports ?? []).forEach(walk);
};
for (const [key, c] of Object.entries(manifest)) if (c.isEntry) walk(key);

// Every emitted file counts. The manifest lists the main build only, not the
// highlighter worker or the grammar chunks it loads.
const assets = readdirSync(join(dist, "assets")).map((f) => `assets/${f}`);
const allJs = assets.filter((f) => f.endsWith(".js"));
const sizes = new Map(allJs.map((f) => [f, gz(f)]));
const initialKb = [...initial].filter((f) => f.endsWith(".js")).reduce((a, f) => a + sizes.get(f), 0);
const totalKb = allJs.reduce((a, f) => a + sizes.get(f), 0);

for (const f of [...allJs].sort((a, b) => sizes.get(b) - sizes.get(a))) {
  console.log(`  ${sizes.get(f).toFixed(1).padStart(6)} KB  ${initial.has(f) ? "initial " : "        "}${f}`);
}
// Stylesheets and fonts come with their chunks but are outside the JS
// budget; they are listed so the whole download is visible. A font is
// compressed already, so its own size is what travels.
for (const f of assets.filter((f) => !f.endsWith(".js")).sort()) {
  const font = /\.(woff2?|ttf|otf)$/.test(f);
  const kb = font ? read(f).length / 1024 : gz(f);
  console.log(`  ${kb.toFixed(1).padStart(6)} KB  ${(font ? "font" : f.split(".").pop()).padEnd(8)}${f}`);
}
console.log(`initial JS: ${initialKb.toFixed(1)} KB gzip (budget ${INITIAL_KB}, ceiling ${PROGRAM_INITIAL_KB})`);
console.log(`total JS:   ${totalKb.toFixed(1)} KB gzip (budget ${TOTAL_KB}, ceiling ${PROGRAM_TOTAL_KB}), ${allJs.length} chunks`);

/** The emitted chunks a lazy part must have; none means it is not lazy any more. */
const chunk = (re, what) => {
  const found = allJs.filter((f) => re.test(f));
  if (found.length === 0) failures.push(`no ${what} chunk in dist/assets`);
  return found;
};
/** A lazy part's chunks: they must exist and stay out of the initial load. */
const lazyChunk = (re, what) => {
  const found = chunk(re, what);
  for (const f of found) if (initial.has(f)) failures.push(`${f} (${what}) is in the initial load`);
  return found;
};
// The mode backgrounds and their host load only when one shows, and the
// still copy beside chats only when it is turned on.
lazyChunk(/\/Backdrop-/, "background host");
lazyChunk(/\/SearchBg-/, "Search background");
lazyChunk(/\/WallBg-/, "Ask background");
lazyChunk(/\/BinaryBg-/, "Agentic background");
lazyChunk(/\/ChatGutter-/, "chat gutter");

// I3: its lazy chunks (the search panel and the account menu).
const lazyI3 = [...lazyChunk(/\/SearchPanel-/, "search panel"), ...lazyChunk(/\/AccountMenu-/, "account menu")];

// I4: its lazy chunks (the area pages, then their previews).
const lazyI4 = [
  ...lazyChunk(/\/Routines-/, "Routines page"),
  ...lazyChunk(/\/Media-/, "Media page"),
  ...lazyChunk(/\/Artifacts-/, "Artifacts page"),
  ...lazyChunk(/\/MediaPreview-/, "Media preview"),
];

console.log(`lazy chunks checked: ${lazyI3.length} (I3), ${lazyI4.length} (I4)`);
// What the home screen and every chat load: math and highlighting must not.
const eager = [...initial, ...chunk(/\/Conversation-/, "Conversation")];
// An installed package's file, found the way the package that pulls it in
// finds it (pnpm keeps transitive dependencies out of web/node_modules).
const installed = (via, file) => {
  try {
    const from = via.reduce((at, dep) => createRequire(at).resolve(dep), join(web, "package.json"));
    return [createRequire(from).resolve(file)].filter(existsSync);
  } catch {
    return [];
  }
};

// Each library is found by a string only it contains, one that survives
// minification. A marker must also match where the library does live (its
// lazy chunk, or its installed package when nothing may bundle it);
// otherwise a check against it would pass without testing anything.
const markers = [
  // micromark-extension-math's HTML half imports KaTeX; it must tree-shake out.
  { lib: "KaTeX", re: /KaTeX parse error/, banned: allJs, home: installed(["remark-math", "micromark-extension-math"], "katex/dist/katex.mjs") },
  // Our own code names mermaid fences, so the marker is the library's API
  // object. Mermaid is not installed, so there is nothing to hold it to.
  { lib: "mermaid", re: /mermaidAPI/, banned: allJs, home: [] },
  // refractor's build has no `Prism.languages`: its core throws "Unknown
  // language" and its grammars call `languages.extend`.
  { lib: "Temml", re: /temml/i, banned: eager, home: chunk(/\/math-/, "math") },
  { lib: "the highlighter", re: /Unknown language|languages\.extend/, banned: eager, home: chunk(/\/highlight\.worker-/, "highlighter worker") },
];
const has = (file, re) => re.test(read(file).toString("utf8"));
for (const m of markers) {
  for (const f of m.banned) if (has(f, m.re)) failures.push(`${f} contains ${m.lib}`);
  if (m.home.length && !m.home.some((f) => has(f, m.re))) failures.push(`the ${m.lib} marker ${m.re} matches nothing in ${m.home.join(", ")}`);
}
// A worker has no DOM: a browser build that needs one breaks highlighting.
for (const f of allJs.filter((f) => /\/highlight\.worker-/.test(f))) {
  if (has(f, /\b(document|window)\./)) failures.push(`${f} uses the DOM, which a worker does not have`);
}
if (initialKb > INITIAL_KB || totalKb > TOTAL_KB) failures.push("bundle budget exceeded");
if (initialKb > PROGRAM_INITIAL_KB) failures.push(`initial JS is over the program ceiling of ${PROGRAM_INITIAL_KB} KB`);
if (totalKb > PROGRAM_TOTAL_KB || TOTAL_KB > PROGRAM_TOTAL_KB) {
  failures.push(`the total or its budget is over the program ceiling of ${PROGRAM_TOTAL_KB} KB`);
}
for (const msg of failures) console.error(msg);
if (failures.length) process.exit(1);

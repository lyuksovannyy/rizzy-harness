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
//
// The total is a soft limit (since 2026-11-08): it records what each feature
// added and why, and a feature that needs room raises its line with the
// reason. Most of it is lazy chunks a visit loads only when it opens their
// page, all of it is cached for a year once loaded, and the server sends it
// gzipped. The hard limits are the ones a visit waits for: the first-load
// ceiling, the lazy-chunk checks and the library markers below.

// Measured total of the last merged phase, with what it bought.
const BASE_KB = 361.0; // measured 360.9 on 2026-11-09 (initial 102.9) after the fix-plan cuts: the JWT session keeper, the server-setup pages and node-problem notices, and dead code are gone

// I2: its own growth over BASE_KB + ≤ 0.5, with the reason on the line.
const I2_KB = 0;

// I3: its own growth over BASE_KB + ≤ 0.5, with the reason on the line.
const I3_KB = 0;

// I4: its own growth over BASE_KB + ≤ 0.5, with the reason on the line.
const I4_KB = 0;

// Fix plan 3.10 (audit L14), measured 361.9 (initial 103.4): drafts kept per
// user and cleared at sign-out, with the chat and for incognito chats (first
// load, +0.5); the server-made temporary password's dialog and per-message
// footnote ids (lazy, +0.4).
const L14_KB = 0.9;

// Gate 3 P-1, measured 363.0 (initial 103.6): the login key kept and sent
// by the API client and the socket (first load, +0.2); the links that
// what the browser loads by itself (<img>, <video>, downloads) needs, in
// their own chunk that only lazy pages and the composer's image chip
// load (+0.9).
const P1_KB = 1.2;

// Gate 3 fix review 2, W-1 to W-3, measured 366.5 (initial 103.9): the
// approval card shows every field of a call in full, with sizes, head and
// tail, blank-run and hidden-character markers, look-alike and path notes
// (lazy, in the conversation chunk, +3.1); hidden characters in chat
// titles and node names shown as codes on the first screen (+0.3).
const R6_KB = 3.5;

// 2026-11, measured 368.2 (initial 99.4): page-only API calls, strings,
// model helpers and project icons moved out of the first load into the
// chunks that use them. The first load fell by 4.6; the total rose by 1.6,
// which is what five more chunks cost (+1.6). Gate 3 R2-1: the favicon is
// fetched with the login key through the links chunk (+0.1).
const SPLIT_KB = 1.7;

// T.8d step 1, measured 368.4 (initial 99.5): the fold knows the two
// background events and the conversation shows a background command's end
// as a plain line with its tail as text (+0.2).
// T.8d step 2, measured 370.5 (initial 99.5, unchanged): the chat's
// background strip (Stop, Stop all, time left, a node away), its own lazy
// chunk loaded on a chat's first running background command (+1.9, its
// stylesheet 0.4 beside it), and the conversation's lazy import of it and
// the fold's timeout (+0.1); the strip's hidden-character marks on a
// refusal took it from 370.4 to 370.5 (+0.1).
const T8D_KB = 2.3;

// Rizzy Glass stage 1, measured 370.6 (initial 99.7): the accent presets'
// one list, in the first load (+0.1).
const GLASS_KB = 0.1;

// N.10a part 1, measured 373.0 on Rizzy Glass (initial 100.1, from 99.7):
// the chat header's mode and gaps (in the lazy conversation chunk), the
// new chat's mode choice and the host shell's warning, each its own lazy
// chunk (only server admins on a node offering the host shell load them),
// and in the first load the composer's mode state and the rule for who may
// pick it (+2.4; 372.7 before the merge, the rest the review's fixes: the
// gaps' words, the choice bound to its node).
const N10A_KB = 2.4;

// N.10a part 2, measured 373.9 (initial 100.1, unchanged): the level
// select and its host warning's grant variant, in the lazy Settings chunk
// (Users, Workspace admin → Members) and the warning's own lazy chunk; the
// composer's rule now reads the level instead of the admin role (+0.9).
// The review's fixes, measured 374.1 (initial 100.2): the member list's
// levels as they count, the server admin's level select and the demotion's
// question, all in the lazy Settings chunk (+0.2).
const N10A2_KB = 1.1;

// The agent's task list (`todo`), measured 374.1 (initial 100.1, unchanged)
// against 373.0 on main: the pinned panel, its own lazy chunk loaded on a
// chat's first list (0.7, its stylesheet 0.5 beside it) and the lucide
// circle icon it shares (0.2); the conversation's lazy import and the tool
// card's line (+0.2); the fold's case in the first load (+0.1 before
// rounding). Its review's fixes, measured 374.3 (initial 100.2): a new
// chat's agent per mode for the home screen, projects and routines (TD-1,
// first load, +0.1) and the panel folding when no turn works on it (TD-2,
// +0.1).
const TODO_KB = 1.3;

// Rounded to 0.01 first, so a float sum (0.1 + 0.2) cannot round a tenth up.
const TOTAL_KB = Math.ceil(Math.round((BASE_KB + I2_KB + I3_KB + I4_KB + L14_KB + P1_KB + R6_KB + SPLIT_KB + T8D_KB + GLASS_KB + N10A_KB + N10A2_KB + TODO_KB) * 100) / 10) / 10;
// Raised from 103 to 106 once the Phase A pages merged (103.7 measured: the
// new sidebar, account button and routine-result rows are on the first
// screen), then lowered to 104 at Phase B (102.3 measured) once the login
// form moved to its own chunk. The rest is for the Phase C image attach in
// the composer (~0.4). Still under the long-standing 110 KB first-load budget.
// Three security fixes had brought it back to 103.9. On 2026-11-11 what only
// lazy pages use left the first load (the page-only API calls, the model plan
// helpers, the page-only English strings, 19 of the project icons: 4.6 KB), so
// the initial load measured 99.3. The ceiling stays 104: the room is for the
// next fixes, not for features.
const PROGRAM_INITIAL_KB = 104;
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
console.log(`total JS:   ${totalKb.toFixed(1)} KB gzip (budget ${TOTAL_KB}, soft), ${allJs.length} chunks`);

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
// T.8d step 2: the chat's background strip loads only while a chat has a
// background command running.
lazyChunk(/\/BackgroundStrip-/, "background strip");
// The agent's task list loads only once a chat has one.
lazyChunk(/\/TodoPanel-/, "task list panel");

// I3: its lazy chunks (the search panel, the account menu, and the login
// form, which signed-in visits never draw).
const lazyI3 = [
  ...lazyChunk(/\/SearchPanel-/, "search panel"),
  ...lazyChunk(/\/AccountMenu-/, "account menu"),
  ...lazyChunk(/\/Login-/, "login form"),
];

// I4: its lazy chunks (the area pages, their previews, and the composer's
// image shrinking, which loads on the first image put in a message).
const lazyI4 = [
  ...lazyChunk(/\/Routines-/, "Routines page"),
  ...lazyChunk(/\/Media-/, "Media page"),
  ...lazyChunk(/\/Artifacts-/, "Artifacts page"),
  ...lazyChunk(/\/MediaPreview-/, "Media preview"),
  ...lazyChunk(/\/downscale-/, "image downscale"),
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
if (initialKb > INITIAL_KB) failures.push("initial JS is over its budget");
// Soft: reported, never a failure (it failed CI on main while called soft).
if (totalKb > TOTAL_KB) {
  console.warn(`warning: total JS is ${totalKb.toFixed(1)} KB, over the recorded ${TOTAL_KB}: raise your line above to it with the reason`);
}
if (initialKb > PROGRAM_INITIAL_KB) failures.push(`initial JS is over the program ceiling of ${PROGRAM_INITIAL_KB} KB`);
for (const msg of failures) console.error(msg);
if (failures.length) process.exit(1);

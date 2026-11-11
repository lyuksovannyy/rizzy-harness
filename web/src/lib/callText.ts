// What an approval card shows of a tool call (W-1 of the Gate 3 fix
// review 2). The rule: whoever clicks Allow has had the whole call in front
// of them as it will run, with nothing left out without saying so and
// nothing disguised. So every key and value of the input is a field; a long
// text shows its beginning and its end and says how much of the middle is
// not on screen; hidden, control and unusual blank characters become codes;
// long runs of blanks become counted markers; a word that mixes scripts and
// a path that leaves the chat's folder are named. Pure functions here, the
// rendering in components/CallInput.tsx. Lazy: only the conversation loads it.
import { SHOWN_SRC, codeOf } from "../md/hiddenChars";

/** `cmd`: a command, a path or a short value, where a few blanks in a row
 * are already odd. `text`: file content or prose, where indentation and
 * paragraph breaks are normal. */
export type Mode = "cmd" | "text";

const RUNS: Record<Mode, { spaces: number; tabs: number; breaks: number }> = {
  cmd: { spaces: 8, tabs: 4, breaks: 3 },
  text: { spaces: 40, tabs: 8, breaks: 6 },
};
const RE: Record<Mode, RegExp> = {
  cmd: runs("cmd"),
  text: runs("text"),
};
function runs(m: Mode): RegExp {
  const r = RUNS[m];
  return new RegExp(`(${SHOWN_SRC})| {${r.spaces},}|\\t{${r.tabs},}|\\n{${r.breaks},}`, "gu");
}
const SHOWN = new RegExp(SHOWN_SRC, "gu");

export const fmt = (n: number) => n.toLocaleString("en-US");
const plural = (n: number, one: string, many: string) => `${fmt(n)} ${n === 1 ? one : many}`;

/** A piece of shown text: as it is, or a marker that stands for something
 * that would not be visible (a hidden character, a run of blanks). */
export interface Seg {
  t: string;
  mark?: true;
}

/** `text` as the card shows it: every character either as itself or inside
 * a marker; nothing dropped. A run of line breaks keeps one real break after
 * its marker, so the lines around it stay lines. */
export function marks(text: string, mode: Mode): Seg[] {
  const out: Seg[] = [];
  let last = 0;
  for (const m of text.matchAll(RE[mode])) {
    const at = m.index;
    const s = m[0];
    if (at > last) out.push({ t: text.slice(last, at) });
    if (m[1] !== undefined) out.push({ t: codeOf(s), mark: true });
    else if (s[0] === " ") out.push({ t: `⟨${plural(s.length, "space", "spaces")}⟩`, mark: true });
    else if (s[0] === "\t") out.push({ t: `⟨${plural(s.length, "tab", "tabs")}⟩`, mark: true });
    else out.push({ t: `⟨${plural(s.length, "line break", "line breaks")}⟩`, mark: true }, { t: "\n" });
    last = at + s.length;
  }
  if (last < text.length) out.push({ t: text.slice(last) });
  return out;
}

/** marks() as one string, for a one-line head. */
export const markedText = (text: string, mode: Mode) =>
  marks(text, mode)
    .map((s) => s.t)
    .join("");

/** Hidden, control and unusual blank characters in `text`. */
export const hiddenCount = (text: string) => text.match(SHOWN)?.length ?? 0;

/** Characters (code points, as a person counts them) and lines. */
export function sizeOf(text: string): { chars: number; lines: number } {
  let chars = 0;
  for (const _ of text) chars++;
  const lines = text === "" ? 0 : (text.match(/\n/g)?.length ?? 0) + 1;
  return { chars, lines };
}

export const sizeText = (s: { chars: number; lines: number }) =>
  `${plural(s.chars, "character", "characters")}, ${plural(s.lines, "line", "lines")}`;

/** Longer than this, a text shows its head and its tail until expanded. */
export const LONG = { chars: 1500, lines: 40 };
const HEAD = { chars: 700, lines: 15 };
const TAIL = { chars: 500, lines: 10 };

/** A long text as shown collapsed: its beginning, its end (an attacker
 * picks what falls in the middle, so the end is shown too) and how much of
 * the middle is not on screen. Null when the text is shown whole. */
export interface Cut {
  head: string;
  tail: string;
  /** Characters and line breaks of the middle part, not on screen. */
  chars: number;
  breaks: number;
}

/** Long is measured as shown: a run of 5,000 blanks is one marker, not a
 * reason to collapse a short command. */
export function cut(text: string, mode: Mode): Cut | null {
  if (text.length <= LONG.chars && !text.includes("\n")) return null;
  const shown = marks(text, mode).reduce((n, s) => n + s.t.length, 0);
  if (shown <= LONG.chars && shownLines(text) <= LONG.lines) return null;
  const h = headEnd(text);
  const t = tailStart(text);
  if (t <= h) return null;
  const mid = text.slice(h, t);
  return { head: text.slice(0, h), tail: text.slice(t), chars: sizeOf(mid).chars, breaks: mid.match(/\n/g)?.length ?? 0 };
}

// Lines as the card shows them: a run of line breaks is one marker and one
// break, so blank-line padding neither counts as many lines nor gets cut.
const lineEnd = (s: string, i: number) => s.charCodeAt(i) === 10 && s.charCodeAt(i + 1) !== 10;
const shownLines = (s: string) => (s.match(/\n+/g)?.length ?? 0) + 1;

// Never between the two halves of a surrogate pair.
const safe = (s: string, i: number) => (i > 0 && i < s.length && /[\uDC00-\uDFFF]/.test(s[i]!) ? i - 1 : i);

function headEnd(s: string): number {
  let end = Math.min(s.length, HEAD.chars);
  let n = 0;
  for (let i = 0; i < end; i++) {
    if (lineEnd(s, i) && ++n === HEAD.lines) {
      end = i + 1;
      break;
    }
  }
  return safe(s, end);
}

function tailStart(s: string): number {
  let start = Math.max(0, s.length - TAIL.chars);
  // A final line break ends the last line rather than starting another.
  const want = TAIL.lines + (s.endsWith("\n") ? 1 : 0);
  let n = 0;
  for (let i = s.length - 1; i >= start; i--) {
    if (lineEnd(s, i) && ++n === want) {
      start = i + 1;
      break;
    }
  }
  return safe(s, start);
}

/** How the middle of a cut is described. */
export const cutText = (c: Cut) =>
  `${plural(c.chars, "character", "characters")}${c.breaks ? ` (${plural(c.breaks, "line break", "line breaks")})` : ""} in the middle not shown`;

// Look-alike letters. Not a confusables table: a word (a run of letters,
// marks and digits) that mixes scripts is named, with the letters that are
// not from its main script, because `сurl` with a Cyrillic с reads as curl.
// A word written wholly in another script is not flagged, nor are the
// Japanese and Korean mixes or Latin next to CJK, which are ordinary and
// not look-alikes.
const SCRIPTS = [
  "Latin",
  "Greek",
  "Cyrillic",
  "Armenian",
  "Georgian",
  "Cherokee",
  "Hebrew",
  "Arabic",
  "Devanagari",
  "Thai",
  "Hangul",
  "Hiragana",
  "Katakana",
  "Han",
].map((n) => [n, new RegExp(`\\p{Script=${n}}`, "u")] as const);
const FULLWIDTH = /[Ａ-Ｚａ-ｚ]/u;
const MATH = /[\u{1D400}-\u{1D7FF}]/u;
const CJK = new Set(["Han", "Hiragana", "Katakana", "Hangul"]);
const LETTER = /\p{L}/u;

function scriptOf(ch: string): string {
  if (FULLWIDTH.test(ch)) return "fullwidth Latin";
  if (MATH.test(ch)) return "mathematical";
  for (const [n, re] of SCRIPTS) if (re.test(ch)) return n;
  return "other";
}

export interface Mixed {
  word: string;
  /** The letters not from the word's main script: `с U+0441 Cyrillic`. */
  odd: string[];
  scripts: string[];
}

export function mixedScripts(text: string): Mixed[] {
  const out: Mixed[] = [];
  for (const word of new Set(text.match(/[\p{L}\p{M}\p{N}]+/gu) ?? [])) {
    if (/^[\x00-\x7f]*$/.test(word)) continue;
    const by = new Map<string, Set<string>>();
    for (const ch of word) {
      if (!LETTER.test(ch)) continue;
      const s = scriptOf(ch);
      if (!by.has(s)) by.set(s, new Set());
      by.get(s)!.add(ch);
    }
    if (by.size < 2) continue;
    const scripts = [...by.keys()];
    if (scripts.every((s) => s === "Latin" || CJK.has(s))) continue;
    const main = by.has("Latin") ? "Latin" : scripts.reduce((a, b) => (by.get(b)!.size > by.get(a)!.size ? b : a));
    const odd: string[] = [];
    for (const [s, chars] of by)
      if (s !== main) for (const ch of chars) odd.push(`${ch} ${codeOf(ch).slice(1, -1)} ${s}`);
    out.push({ word, odd: odd.slice(0, 8), scripts });
    if (out.length >= 10) break;
  }
  return out;
}

/** What a written path makes knowable: absolute, outside the chat's folder
 * (lexically; the node resolves links and refuses what its policy does not
 * allow), or climbing out of it with `..`. */
export function pathNotes(p: string, cwd?: string): string[] {
  if (p === "") return ["The path is empty."];
  const notes: string[] = [];
  if (p.startsWith("/") || /^[A-Za-z]:[\\/]/.test(p) || p.startsWith("\\\\")) {
    notes.push("This is an absolute path.");
    if (cwd?.startsWith("/") && p.startsWith("/") && !inside(norm(p), norm(cwd)))
      notes.push(`It is outside this chat's folder, ${cwd}.`);
  } else if (climbs(p)) {
    notes.push("It leaves this chat's folder through “..”.");
  }
  if (p === "~" || p.startsWith("~/")) notes.push("“~” is not expanded: this is a folder named ~ inside the chat's folder.");
  return notes;
}

function norm(p: string): string[] {
  const out: string[] = [];
  for (const s of p.split("/")) {
    if (s === "" || s === ".") continue;
    if (s === "..") out.pop();
    else out.push(s);
  }
  return out;
}

const inside = (p: string[], dir: string[]) => dir.every((s, i) => p[i] === s);

function climbs(p: string): boolean {
  let depth = 0;
  for (const s of p.split("/")) {
    if (s === "" || s === ".") continue;
    depth += s === ".." ? -1 : 1;
    if (depth < 0) return true;
  }
  return false;
}

/** One line of the card's account of the input. `key` is unique per field
 * (the JSON path), `label` is what the person reads. */
export type Field =
  | { kind: "text"; key: string; label: string; depth: number; value: string; mode: Mode; tone?: "del" | "add"; cut: Cut | null }
  | { kind: "path"; key: string; label: string; depth: number; value: string }
  | { kind: "value"; key: string; label: string; depth: number; value: string; type: string }
  | { kind: "group"; key: string; label: string; depth: number; summary: string };

type Special = { key: string; label: string; as: Mode | "path"; tone?: "del" | "add" };

// Tools whose main arguments get a name; every other key still follows.
const SPECIAL: Record<string, Special[]> = {
  bash: [{ key: "command", label: "Command", as: "cmd" }],
  write: [
    { key: "path", label: "Path", as: "path" },
    { key: "content", label: "Content", as: "text" },
  ],
  edit: [
    { key: "path", label: "Path", as: "path" },
    { key: "old_string", label: "Replace this", as: "text", tone: "del" },
    { key: "new_string", label: "With this", as: "text", tone: "add" },
  ],
  delegate: [
    { key: "agent", label: "Agent", as: "cmd" },
    { key: "task", label: "Task", as: "text" },
  ],
  publish: [{ key: "path", label: "File to upload", as: "path" }],
};
/** Keys the web tools' own line (WebInput) shows in full. */
const WEB_SHOWN: Record<string, string> = { web_fetch: "url", web_search: "query" };

const keyOf = (path: (string | number)[]) => JSON.stringify(path);

/** Every key and value of `input`, the named ones first, the rest in the
 * order the event holds them. A new tool gets the generic fields: never
 * less than its whole input. */
export function fields(tool: string, input: unknown): Field[] {
  const out: Field[] = [];
  if (!input || typeof input !== "object" || Array.isArray(input)) {
    walk(out, [], "input", input, 0);
    return out;
  }
  const rec = input as Record<string, unknown>;
  const done = new Set<string>();
  if (WEB_SHOWN[tool] !== undefined && WEB_SHOWN[tool] in rec) done.add(WEB_SHOWN[tool]);
  const special = [...(SPECIAL[tool] ?? [])];
  if (!special.some((s) => s.key === "path")) special.push({ key: "path", label: "path", as: "path" });
  for (const s of special) {
    const v = rec[s.key];
    if (typeof v !== "string" || !Object.hasOwn(rec, s.key)) continue;
    done.add(s.key);
    const key = keyOf([s.key]);
    if (s.as === "path") out.push({ kind: "path", key, label: s.label, depth: 0, value: v });
    else out.push({ kind: "text", key, label: s.label, depth: 0, value: v, mode: s.as, tone: s.tone, cut: cut(v, s.as) });
  }
  for (const [k, v] of Object.entries(rec)) if (!done.has(k)) walk(out, [k], k, v, 0);
  return out;
}

function walk(out: Field[], path: (string | number)[], label: string, v: unknown, depth: number): void {
  const key = keyOf(path);
  if (typeof v === "string") {
    const mode: Mode = v.length <= 300 && !v.includes("\n") ? "cmd" : "text";
    out.push({ kind: "text", key, label, depth, value: v, mode, cut: cut(v, mode) });
  } else if (Array.isArray(v)) {
    out.push({ kind: "group", key, label, depth, summary: v.length ? `list of ${plural(v.length, "item", "items")}` : "empty list" });
    v.forEach((x, i) => walk(out, [...path, i], `[${i}]`, x, depth + 1));
  } else if (v !== null && typeof v === "object") {
    const entries = Object.entries(v);
    out.push({ kind: "group", key, label, depth, summary: entries.length ? plural(entries.length, "field", "fields") : "empty object" });
    for (const [k, x] of entries) walk(out, [...path, k], k, x, depth + 1);
  } else {
    out.push({ kind: "value", key, label, depth, value: v === undefined ? "undefined" : JSON.stringify(v), type: v === null ? "null" : typeof v });
  }
}

/** Characters of the input that are not on screen with these fields open. */
export function unseen(list: Field[], open: ReadonlySet<string>): number {
  let n = 0;
  for (const f of list) if (f.kind === "text" && f.cut && !open.has(f.key)) n += f.cut.chars;
  return n;
}

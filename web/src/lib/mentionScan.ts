// Which agents a person's message calls (X.37, README "Calling several
// agents"): the exact mirror of `crates/core/src/mention.rs`, used only to
// DRAW the message and the composer (the server decides who answers). Both
// read `src/__fixtures__/mentions.json`.
//
// A message calls the agents named in its opening run (the mentions it begins
// with, separated by spaces, newlines and/or commas) and in its closing line
// (a last line that holds nothing but resolved mentions). A mention anywhere
// else is a plain mention. A mention is `@` at the start of a token and a
// handle `[a-z0-9-]+` ended by the end, a space, `,` or `:`; it counts only
// when `resolve` knows it. Nothing in a fenced block or an inline code span
// counts.

export const MAX_CALLED = 4;

/** What a handle names: `agent:<id>` or `person`; null: nothing (text). */
export type Resolve = (name: string) => string | null;

export interface Found {
  start: number;
  end: number;
  who: string;
  trigger: boolean;
}

export interface Mentions {
  found: Found[];
  /** The agents called, each once, in order (at most MAX_CALLED + 1). */
  called: string[];
}

// Rust's `char::is_whitespace` (Unicode White_Space), not JavaScript's `\s`.
const WS = "\\t-\\r \\u0085\\u00a0\\u1680\\u2000-\\u200a\\u2028\\u2029\\u202f\\u205f\\u3000";
const IS_WS = new RegExp(`^[${WS}]$`);
const LEAD_WS = new RegExp(`^[${WS}]*`);
const TRAIL_WS = new RegExp(`[${WS}]*$`);
const LEAD_SEP = new RegExp(`^[${WS},]*`);

const isWs = (c: string) => IS_WS.test(c);
const isSep = (c: string) => c === "," || IS_WS.test(c);
const trimStart = (s: string) => s.slice(LEAD_WS.exec(s)![0].length);
const trimEnd = (s: string) => s.slice(0, TRAIL_WS.exec(s)!.index);

function handleByte(c: number): boolean {
  return (c >= 97 && c <= 122) || (c >= 48 && c <= 57) || c === 45;
}

function mentionAt(text: string, at: number, resolve: Resolve): [number, string] | null {
  let end = at + 1;
  while (end < text.length && handleByte(text.charCodeAt(end))) end++;
  if (end === at + 1) return null;
  const next = text[end];
  if (next !== undefined && !(isWs(next) || next === "," || next === ":")) return null;
  const who = resolve(text.slice(at + 1, end));
  return who === null ? null : [end, who];
}

function openingRun(text: string, resolve: Resolve): number[] {
  const out: number[] = [];
  let at = text.length - trimStart(text).length;
  while (text[at] === "@") {
    const m = mentionAt(text, at, resolve);
    if (!m) break;
    out.push(at);
    const end = m[0];
    if (text[end] === ":") break;
    at = end + LEAD_SEP.exec(text.slice(end))![0].length;
  }
  return out;
}

function closingLine(text: string, lineStart: number, resolve: Resolve): number[] {
  const endOfText = trimEnd(text).length;
  const out: number[] = [];
  let at = lineStart;
  for (;;) {
    at += LEAD_SEP.exec(text.slice(at, endOfText))![0].length;
    if (at >= endOfText) break;
    if (text[at] !== "@") return [];
    const m = mentionAt(text, at, resolve);
    // `:` is not a separator in a closing line.
    if (!m || text[m[0]] === ":") return [];
    out.push(at);
    at = m[0];
  }
  return out;
}

function scanLine(text: string, from: number, to: number, resolve: Resolve, out: Found[]) {
  let i = from;
  let tokenStart = true;
  while (i < to) {
    const c = text[i]!;
    if (c === "`") {
      let n = 0;
      while (i + n < to && text[i + n] === "`") n++;
      // A closing run of the same length, later on this line.
      let j = i + n;
      let closed = -1;
      while (j < to) {
        if (text[j] === "`") {
          let m = 0;
          while (j + m < to && text[j + m] === "`") m++;
          if (m === n) {
            closed = j + m;
            break;
          }
          j += m;
        } else j++;
      }
      i = closed >= 0 ? closed : i + n;
      tokenStart = false;
    } else if (c === "@" && tokenStart) {
      const m = mentionAt(text, i, resolve);
      if (m) {
        out.push({ start: i, end: m[0], who: m[1], trigger: false });
        i = m[0];
      } else i++;
      tokenStart = false;
    } else {
      tokenStart = isSep(c);
      i++;
    }
  }
}

/** Every resolved mention outside code, and the agents called. */
export function scanMentions(text: string, resolve: Resolve): Mentions {
  const found: Found[] = [];
  let inFence = false;
  let last: [number, boolean] = [0, false];
  let lineStart = 0;
  const bodyEnd = trimEnd(text).length;
  for (const line of text.split(/(?<=\n)/)) {
    const start = lineStart;
    lineStart += line.length;
    const t = trimStart(line);
    const fence = t.startsWith("```") || t.startsWith("~~~");
    if (start < bodyEnd) last = [start, inFence || fence];
    if (fence) {
      inFence = !inFence;
      continue;
    }
    if (inFence) continue;
    scanLine(text, start, start + line.length, resolve, found);
  }
  const marks = new Set<number>(openingRun(text, resolve));
  if (trimStart(text) !== "" && !last[1]) for (const s of closingLine(text, last[0], resolve)) marks.add(s);
  const called: string[] = [];
  for (const f of found) {
    f.trigger = marks.has(f.start);
    if (f.trigger && f.who.startsWith("agent:")) {
      const id = f.who.slice(6);
      if (!called.includes(id)) {
        called.push(id);
        if (called.length > MAX_CALLED) break;
      }
    }
  }
  return { found, called };
}

/** The resolver the UI has: the chat's agents by id or alias, and the signed-in person's own login. */
export function resolver(agents: { id: string; aliases?: string[] }[], login?: string): Resolve {
  const by = new Map<string, string>();
  for (const a of agents) for (const h of [...(a.aliases ?? []), a.id]) by.set(h, a.id);
  // An id beats an alias, as the server's set resolves.
  for (const a of agents) by.set(a.id, a.id);
  return (name) => (by.has(name) ? `agent:${by.get(name)}` : name === login ? "person" : null);
}

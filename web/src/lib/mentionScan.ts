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
/** The most resolved mentions one message may hold (the server refuses more). */
export const MAX_RESOLVED = 16;

/** What a handle names: `agent:<id>`, `tag:<id>` (an agent that runs only as a sub-agent: mentioned, never called) or `person`; null: nothing (text). */
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

// Rust's `char::is_whitespace` (Unicode White_Space), not JavaScript's `\s`
// or `String.trimEnd` (U+FEFF in, U+0085 out). Loops over the characters, not
// regular expressions: an `[ws]*$` backtracks over every run of white space
// (quadratic; a pasted run of 80,000 spaces froze the tab for seconds).
const WS = new Set(
  "\t\n\v\f\r \u0085\u00a0\u1680\u2028\u2029\u202f\u205f\u3000".split("").concat(Array.from({ length: 11 }, (_, i) => String.fromCharCode(0x2000 + i))),
);
const isWs = (c: string) => WS.has(c);
const isSep = (c: string) => c === "," || WS.has(c);
/** The index of the first character at or after `from` that is not white space (`to` at most). */
function skipWs(s: string, from: number, to = s.length): number {
  let i = from;
  while (i < to && WS.has(s[i]!)) i++;
  return i;
}
/** The same for white space and commas. */
function skipSep(s: string, from: number, to = s.length): number {
  let i = from;
  while (i < to && isSep(s[i]!)) i++;
  return i;
}
const trimStart = (s: string) => s.slice(skipWs(s, 0));
function trimEndLen(s: string): number {
  let n = s.length;
  while (n > 0 && WS.has(s[n - 1]!)) n--;
  return n;
}

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
  let at = skipWs(text, 0);
  while (text[at] === "@") {
    const m = mentionAt(text, at, resolve);
    if (!m) break;
    out.push(at);
    const end = m[0];
    if (text[end] === ":") break;
    at = skipSep(text, end);
  }
  return out;
}

function closingLine(text: string, lineStart: number, resolve: Resolve): number[] {
  const endOfText = trimEndLen(text);
  const out: number[] = [];
  let at = lineStart;
  for (;;) {
    at = skipSep(text, at, endOfText);
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
  const bodyEnd = trimEndLen(text);
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
  if (skipWs(text, 0) < text.length && !last[1]) for (const s of closingLine(text, last[0], resolve)) marks.add(s);
  const called: string[] = [];
  // Every mention is flagged, as Rust does; only the list stops at the limit.
  for (const f of found) f.trigger = !f.who.startsWith("tag:") && marks.has(f.start);
  for (const f of found) {
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

/** The sub-agent-only agents `m` mentions, each once, in order. */
export function tagsOf(m: Mentions): string[] {
  const out: string[] = [];
  for (const f of m.found) if (f.who.startsWith("tag:") && !out.includes(f.who.slice(4))) out.push(f.who.slice(4));
  return out;
}

/** The resolver the UI has: the chat's agents by id or alias (those that may answer, and those that run only as sub-agents: tags; one that is neither is text), and the signed-in person's own login. */
export function resolver(agents: { id: string; aliases?: string[]; features?: { as_agent?: boolean; as_subagent?: boolean } }[], login?: string): Resolve {
  const by = new Map<string, string>();
  const kind = (a: (typeof agents)[number]) => (a.features?.as_agent !== false ? "agent" : a.features?.as_subagent !== false ? "tag" : null);
  for (const a of agents) for (const h of [...(a.aliases ?? []), a.id]) if (kind(a)) by.set(h, `${kind(a)}:${a.id}`);
  // An id beats an alias, as the server's set resolves.
  for (const a of agents) if (kind(a)) by.set(a.id, `${kind(a)}:${a.id}`);
  return (name) => by.get(name) ?? (name === login ? "person" : null);
}

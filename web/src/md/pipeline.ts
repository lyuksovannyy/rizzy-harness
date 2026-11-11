// What happens to model text before Markdown parsing, and which URLs survive.
import { stripHidden } from "./hidden";

/** The Markdown we render and copy: hidden characters out, line endings
 * LF, math normalised. (rehypeStripHidden repeats the strip after parsing,
 * for entities.) A ui fence's body then reads exactly as the text Rust
 * hashed (crates/core/src/ui/scan.rs normalize), which micromark would
 * otherwise give with its `\r\n` kept. */
export function prepare(text: string): string {
  return normalizeMath(stripHidden(text).replace(/\r\n?/g, "\n"));
}

// A fence line: optional indent and blockquote markers, then ``` or ~~~.
// Any indent counts, so fences inside list items are skipped too (skipping
// too much only leaves some math as written).
const FENCE = /^\s*(?:>\s*)*(`{3,}|~{3,})(.*)$/;
const DISPLAY_OPEN = /^(\s*(?:>\s*)*)\\\[\s*$/;
const DISPLAY_CLOSE = /^(\s*(?:>\s*)*)\\\]\s*$/;
const DISPLAY_ONE_LINE = /^(\s*(?:>\s*)*)\\\[(.+)\\\]\s*$/;
// `\[x^2\]` on one line is math only if it looks like TeX (a command, a
// script or an equals sign): `\[1\]` is Markdown for a literal [1].
const TEX_HINT = /\\[A-Za-z]|[\^_=]/;
// \( … \) on one line, not preceded by another backslash (an escaped \\).
const INLINE = /(?<!\\)\\\((.+?)\\\)/g;
const QUOTE = /^(?:\s*>)+ ?/;
const LIST_ITEM = /^( *)([-+*]|\d{1,9}[.)])( {1,4}|$)/;
const HEADING = /^ {0,3}#{1,6}(\s|$)/;

/**
 * Turn LaTeX delimiters into the dollar syntax remark-math reads: `\(..\)`
 * becomes inline math, and a `\[` … `\]` pair on their own lines becomes a
 * `$$` block. Fenced and indented code and code spans (even across lines)
 * are left alone, and a single `$` is never math, so prices and `$HOME`
 * stay text. The block structure is tracked roughly; where it is unsure it
 * skips, which only leaves some math as written.
 */
export function normalizeMath(text: string): string {
  if (!text.includes("\\")) return text;
  const lines = text.split("\n");
  const out: string[] = [];
  let para: string[] = [];
  const flush = () => {
    if (para.length) out.push(inlineMath(para.join("\n")));
    para = [];
  };
  let fence: string | null = null;
  // Could an indented line start indented code here? Not inside a paragraph.
  let codeOk = true;
  // The content column of the list item we are in, or -1.
  let listIndent = -1;
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i]!;
    if (fence) {
      out.push(line);
      const f = fenceOf(line);
      // A closer uses the same character, a run at least as long, nothing after.
      if (f && f[1]![0] === fence[0] && f[1]!.length >= fence.length && f[2]!.trim() === "") {
        fence = null;
        codeOk = true;
      }
      continue;
    }
    const inner = line.replace(QUOTE, "");
    if (inner.trim() === "") {
      flush();
      out.push(line);
      codeOk = true;
      continue;
    }
    const indent = columns(inner);
    const item = LIST_ITEM.exec(inner);
    // After a blank line, text left of the item's content ends the list.
    if (codeOk && !item && listIndent >= 0 && indent < listIndent) listIndent = -1;
    if (codeOk && indent >= Math.max(listIndent, 0) + 4) {
      flush();
      out.push(line); // indented code (it goes on until a dedent)
      continue;
    }
    if (item) listIndent = item[1]!.length + item[2]!.length + Math.max(1, item[3]!.length);
    const f = fenceOf(line);
    if (f) {
      flush();
      out.push(line);
      fence = f[1]!;
      continue;
    }
    const one = DISPLAY_ONE_LINE.exec(line);
    if (one && TEX_HINT.test(one[2]!)) {
      flush();
      const q = one[1]!;
      out.push(`${q}$$\n${q}${one[2]!.trim()}\n${q}$$`);
      codeOk = true;
      continue;
    }
    const open = DISPLAY_OPEN.exec(line);
    // Only a block that closes becomes math; an unclosed `$$` would swallow
    // the rest of the message.
    const close = open ? findDisplayClose(lines, i + 1) : -1;
    if (open && close > 0) {
      flush();
      out.push(`${open[1]}$$`, ...lines.slice(i + 1, close), `${DISPLAY_CLOSE.exec(lines[close]!)![1]}$$`);
      i = close;
      codeOk = true;
      continue;
    }
    if (HEADING.test(inner)) {
      flush();
      out.push(inlineMath(line));
      codeOk = true;
      continue;
    }
    para.push(line);
    codeOk = false;
  }
  flush();
  return out.join("\n");
}

function fenceOf(line: string): RegExpExecArray | null {
  const m = FENCE.exec(line);
  // ```a``` on one line is a code span: a backtick info string has no backticks.
  return m && !(m[1]![0] === "`" && m[2]!.includes("`")) ? m : null;
}

/** Leading indent in columns (a tab goes to the next multiple of 4). */
function columns(line: string): number {
  let n = 0;
  for (const c of line) {
    if (c === " ") n++;
    else if (c === "\t") n += 4 - (n % 4);
    else break;
  }
  return n;
}

function findDisplayClose(lines: string[], from: number): number {
  for (let j = from; j < lines.length; j++) {
    if (DISPLAY_CLOSE.test(lines[j]!)) return j;
    if (FENCE.test(lines[j]!)) return -1;
  }
  return -1;
}

/**
 * `\(..\)` to inline math in one paragraph. The dollar run is one longer
 * than any run already there, so a literal `$$` cannot pair with ours (a
 * math span closes only on a run of the same length), and a `$` right next
 * to ours gets a space so the runs do not merge.
 */
function inlineMath(par: string): string {
  if (!par.includes("\\(")) return par;
  const d = "$".repeat(Math.max(2, longestRun(par, "$") + 1));
  return outsideCodeSpans(par, (s) =>
    s.replace(INLINE, (m: string, tex: string, at: number) => {
      // After another conversion (\(a\)\(b\)) or a literal `$`.
      const before = s[at - 1] === "$" || s.slice(at - 2, at) === "\\)" ? " " : "";
      const after = s[at + m.length] === "$" ? " " : "";
      const body = tex.startsWith("$") || tex.endsWith("$") ? ` ${tex} ` : tex;
      return `${before}${d}${body}${d}${after}`;
    }),
  );
}

function longestRun(s: string, ch: string): number {
  let best = 0;
  let run = 0;
  for (const c of s) {
    run = c === ch ? run + 1 : 0;
    best = Math.max(best, run);
  }
  return best;
}

/** Apply `fn` to the parts of a text that are not inside a code span. */
function outsideCodeSpans(text: string, fn: (s: string) => string): string {
  if (!text.includes("`")) return fn(text);
  let out = "";
  let plain = 0;
  let i = 0;
  while (i < text.length) {
    if (text[i] !== "`") {
      i++;
      continue;
    }
    const run = runEnd(text, i) - i;
    const close = findRun(text, i + run, run);
    if (close < 0) {
      i += run; // unmatched backticks are literal text
      continue;
    }
    out += fn(text.slice(plain, i)) + text.slice(i, close + run);
    i = plain = close + run;
  }
  return out + fn(text.slice(plain));
}

function runEnd(text: string, i: number): number {
  while (text[i] === "`") i++;
  return i;
}

function findRun(text: string, from: number, len: number): number {
  for (let k = from; k < text.length; ) {
    if (text[k] !== "`") {
      k++;
      continue;
    }
    const end = runEnd(text, k);
    if (end - k === len) return k;
    k = end;
  }
  return -1;
}

const SAFE_SCHEME = /^(?:https?|mailto):/i;

/**
 * react-markdown's URL hook. Only http(s), mailto and in-page fragments
 * survive (Link keeps a fragment a link only when it names one of the same
 * message's footnotes); anything else becomes "" and the link renders as
 * text. A data: image keeps only its scheme, so the chip can say what it was
 * without the payload going anywhere.
 */
export function urlTransform(url: string, key: string): string {
  const u = url.trim();
  if (key === "src" && /^data:/i.test(u)) return "data:";
  if (u.startsWith("#")) return u;
  return SAFE_SCHEME.test(u) ? u : "";
}

// What happens to model text before Markdown parsing, and which URLs survive.

// Bidi controls, zero-width characters, the BOM, private-use and tag
// characters. They hide or reorder text, so what the user reads (and what
// Copy gives them) would differ from what is there; tag characters are the
// usual carrier for invisible prompt-injection text.
const HIDDEN = /[​-‏‪-‮⁠-⁤⁦-⁩﻿-\u{E0000}-\u{E007F}]/gu;

export function stripHidden(text: string): string {
  return text.replace(HIDDEN, "");
}

/** The Markdown we render and copy: hidden characters out, math normalised. */
export function prepare(text: string): string {
  return normalizeMath(stripHidden(text));
}

// A fence line: optional indent and blockquote markers, then ``` or ~~~.
// Any indent counts, so fences inside list items are skipped too (skipping
// too much only leaves some math as written).
const FENCE = /^\s*(?:>\s*)*(`{3,}|~{3,})(.*)$/;
const DISPLAY_OPEN = /^(\s*(?:>\s*)*)\\\[\s*$/;
const DISPLAY_CLOSE = /^(\s*(?:>\s*)*)\\\]\s*$/;
const DISPLAY_ONE_LINE = /^(\s*(?:>\s*)*)\\\[(.+)\\\]\s*$/;
// \( … \) on one line, not preceded by another backslash (an escaped \\).
const INLINE = /(?<!\\)\\\((.+?)\\\)/g;

/**
 * Turn LaTeX delimiters into the `$$` syntax remark-math reads: `\(..\)`
 * becomes inline `$$..$$`, and a `\[` … `\]` pair on their own lines becomes
 * a `$$` block. Fenced code and code spans are left alone, and a single `$`
 * is never math, so prices and `$HOME` stay text.
 */
export function normalizeMath(text: string): string {
  if (!text.includes("\\")) return text;
  const lines = text.split("\n");
  let fence: string | null = null;
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i]!;
    const m = FENCE.exec(line);
    // ```a``` on one line is a code span: a backtick info string has no backticks.
    const f = m && !(m[1]![0] === "`" && m[2]!.includes("`")) ? m : null;
    if (fence) {
      // A closer uses the same character, a run at least as long, nothing after.
      if (f && f[1]![0] === fence[0] && f[1]!.length >= fence.length && f[2]!.trim() === "") fence = null;
      continue;
    }
    if (f) {
      fence = f[1]!;
      continue;
    }
    const one = DISPLAY_ONE_LINE.exec(line);
    if (one) {
      const q = one[1]!;
      lines[i] = `${q}$$\n${q}${one[2]!.trim()}\n${q}$$`;
      continue;
    }
    const open = DISPLAY_OPEN.exec(line);
    if (open) {
      // Only a block that closes becomes math; an unclosed `$$` would swallow
      // the rest of the message.
      const close = findDisplayClose(lines, i + 1);
      if (close > 0) {
        lines[i] = `${open[1]}$$`;
        lines[close] = `${DISPLAY_CLOSE.exec(lines[close]!)![1]}$$`;
        i = close;
      }
      continue;
    }
    // Adjacent \(a\)\(b\) would give one `$$$$` run, so they get a space.
    lines[i] = outsideCodeSpans(line, (s) =>
      s.replace(INLINE, (_m, tex: string, at: number) => `${s.slice(at - 2, at) === "\\)" ? " " : ""}$$${tex}$$`),
    );
  }
  return lines.join("\n");
}

function findDisplayClose(lines: string[], from: number): number {
  for (let j = from; j < lines.length; j++) {
    if (DISPLAY_CLOSE.test(lines[j]!)) return j;
    if (FENCE.test(lines[j]!)) return -1;
  }
  return -1;
}

/** Apply `fn` to the parts of a line that are not inside a code span. */
function outsideCodeSpans(line: string, fn: (s: string) => string): string {
  if (!line.includes("`")) return fn(line);
  let out = "";
  let plain = 0;
  let i = 0;
  while (i < line.length) {
    if (line[i] !== "`") {
      i++;
      continue;
    }
    const run = runEnd(line, i) - i;
    const close = findRun(line, i + run, run);
    if (close < 0) {
      i += run; // unmatched backticks are literal text
      continue;
    }
    out += fn(line.slice(plain, i)) + line.slice(i, close + run);
    i = plain = close + run;
  }
  return out + fn(line.slice(plain));
}

function runEnd(line: string, i: number): number {
  while (line[i] === "`") i++;
  return i;
}

function findRun(line: string, from: number, len: number): number {
  for (let k = from; k < line.length; ) {
    if (line[k] !== "`") {
      k++;
      continue;
    }
    const end = runEnd(line, k);
    if (end - k === len) return k;
    k = end;
  }
  return -1;
}

const SAFE_SCHEME = /^(?:https?|mailto):/i;

/**
 * react-markdown's URL hook. Only http(s), mailto and in-page fragments
 * (GFM footnotes) survive; anything else becomes "" and the link renders as
 * text. A data: image keeps only its scheme, so the chip can say what it was
 * without the payload going anywhere.
 */
export function urlTransform(url: string, key: string): string {
  const u = url.trim();
  if (key === "src" && /^data:/i.test(u)) return "data:";
  if (u.startsWith("#")) return u;
  return SAFE_SCHEME.test(u) ? u : "";
}

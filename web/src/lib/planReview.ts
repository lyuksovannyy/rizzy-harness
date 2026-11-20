// Reviewing a proposed plan (ADOPTION_PLAN X.7): the plan's Markdown as
// blocks with stable ids, the notes a person writes against them, the block
// diff between two versions of a plan, and the one message that carries the
// notes back. All pure except the small in-memory note store at the end.
// Nothing here talks to the server: the feedback is an ordinary message.
import { fnv1a } from "./fnv";

export type BlockKind = "heading" | "paragraph" | "list" | "code" | "table";

export interface Block {
  /** Stable: the kind, a hash of the text, and which copy of that text it is. */
  id: string;
  kind: BlockKind;
  /** The block's Markdown source, dedented. */
  text: string;
  /** A list item's nesting (two spaces a level). */
  depth: number;
}

const FENCE = /^ {0,3}(`{3,}|~{3,})/;
const HEADING = /^ {0,3}#{1,6}(\s|$)/;
const ITEM = /^(\s*)([-*+]|\d{1,9}[.)])\s+\S/;
const TABLE_RULE = /^\s*\|?\s*:?-+:?\s*(\|\s*:?-+:?\s*)*\|?\s*$/;
const RULE = /^ {0,3}([-*_])( *\1){2,} *$/;

const indentOf = (l: string) => /^\s*/.exec(l)![0].length;

/** The lines from `at` (an opening fence) to its closing fence, inclusive. */
function fenceEnd(lines: string[], at: number): number {
  const open = FENCE.exec(lines[at]!)![1]!;
  for (let j = at + 1; j < lines.length; j++) {
    const m = FENCE.exec(lines[j]!);
    if (m && m[1]![0] === open[0] && m[1]!.length >= open.length && lines[j]!.trim() === m[1]) return j;
  }
  return lines.length - 1;
}

function dedent(lines: string[]): string {
  const n = indentOf(lines[0]!);
  return lines.map((l) => l.slice(Math.min(n, indentOf(l)))).join("\n");
}

/** The plan's Markdown split into blocks: headings, paragraphs (and
 * quotes), list items (each its own block, nested ones too), code fences
 * and tables. Blank lines and rules separate and are not blocks. */
export function splitPlan(markdown: string): Block[] {
  const lines = markdown.replace(/\r\n?/g, "\n").split("\n");
  const raw: { kind: BlockKind; text: string; depth: number }[] = [];
  let i = 0;
  while (i < lines.length) {
    const line = lines[i]!;
    if (!line.trim() || RULE.test(line)) {
      i++;
    } else if (FENCE.test(line)) {
      const end = fenceEnd(lines, i);
      raw.push({ kind: "code", text: dedent(lines.slice(i, end + 1)), depth: 0 });
      i = end + 1;
    } else if (HEADING.test(line)) {
      raw.push({ kind: "heading", text: line.trim(), depth: 0 });
      i++;
    } else if (ITEM.test(line)) {
      const start = i;
      const depth = Math.floor(indentOf(line) / 2);
      i++;
      while (i < lines.length && lines[i]!.trim() && !ITEM.test(lines[i]!) && !HEADING.test(lines[i]!) && indentOf(lines[i]!) > indentOf(line)) {
        i = FENCE.test(lines[i]!) ? fenceEnd(lines, i) + 1 : i + 1;
      }
      raw.push({ kind: "list", text: dedent(lines.slice(start, i)), depth });
    } else if (line.includes("|") && i + 1 < lines.length && TABLE_RULE.test(lines[i + 1]!) && lines[i + 1]!.includes("-")) {
      const start = i;
      i += 2;
      while (i < lines.length && lines[i]!.trim() && lines[i]!.includes("|")) i++;
      raw.push({ kind: "table", text: lines.slice(start, i).join("\n"), depth: 0 });
    } else {
      const start = i;
      i++;
      while (i < lines.length && lines[i]!.trim() && !HEADING.test(lines[i]!) && !FENCE.test(lines[i]!) && !ITEM.test(lines[i]!)) i++;
      raw.push({ kind: "paragraph", text: dedent(lines.slice(start, i)), depth: 0 });
    }
  }
  const seen = new Map<string, number>();
  return raw.map((b) => {
    const key = `${b.kind}-${fnv1a(b.text).toString(36)}`;
    const n = seen.get(key) ?? 0;
    seen.set(key, n + 1);
    return { ...b, id: `${key}-${n}` };
  });
}

export type NoteKind = "comment" | "replace" | "remove";

export interface Note {
  id: string;
  /** The plan (the message id of the agent's answer) it was written for. */
  planId: string;
  blockId: string;
  /** The passage it refers to: the selected text, or the block's source. */
  quote: string;
  /** The quote is a selection inside the block, not the whole block. */
  selection: boolean;
  kind: NoteKind;
  /** The comment, or the replacement text; empty for a removal. */
  text: string;
  /** Carried in a message already. */
  sent: boolean;
}

/** `> ` before every line of `s`. */
function quoted(s: string): string {
  return s
    .split("\n")
    .map((l) => (l ? `> ${l}` : ">"))
    .join("\n");
}

/** A fence that no line of `s` can close. */
function fenced(s: string): string {
  const longest = Math.max(0, ...(s.match(/[`~]+/g) ?? []).map((r) => r.length));
  const f = "`".repeat(Math.max(3, longest + 1));
  return `${f}\n${s}\n${f}`;
}

const KIND_WORDS: Record<NoteKind, string> = { comment: "comment", replace: "replace", remove: "remove" };

/** The one message "Send feedback" sends: a fixed lead, then each note as a
 * quote of the passage and what the person wants done with it. The person
 * reads and may edit this before it is sent. */
export function feedbackText(notes: readonly Note[]): string {
  const out = [
    "Feedback on your plan. Revise it to address each note below, then end your turn with the full revised plan. Do not carry anything out yet.",
  ];
  notes.forEach((n, i) => {
    const parts = [`**Note ${i + 1} of ${notes.length} (${KIND_WORDS[n.kind]})**`, quoted(n.quote)];
    if (n.kind === "comment") {
      parts.push(n.text.includes("\n") || /[`~]{3}/.test(n.text) ? `Comment:\n${fenced(n.text)}` : `Comment: ${n.text}`);
    } else if (n.kind === "replace") {
      parts.push(`Replace the quoted passage with:\n${fenced(n.text)}`);
    } else {
      parts.push("Remove the quoted passage.");
    }
    out.push(parts.join("\n\n"));
  });
  return out.join("\n\n");
}

/** `notes` in the order of the plan's blocks (a selection keeps the order it was written in). */
export function inPlanOrder(notes: readonly Note[], blocks: readonly Block[]): Note[] {
  const at = (n: Note) => {
    const i = blocks.findIndex((b) => b.id === n.blockId);
    return i < 0 ? Infinity : i;
  };
  return notes.map((n, i) => ({ n, i })).sort((a, b) => at(a.n) - at(b.n) || a.i - b.i).map((x) => x.n);
}

export type DiffStatus = "same" | "added" | "removed" | "changed";
export interface DiffRow {
  status: DiffStatus;
  /** The block as it is in the newer plan (the older one for `removed`). */
  block: Block;
  /** `changed`: the older block it replaced. */
  before?: Block;
}

/** Block-level diff of two versions of a plan: the longest common run of
 * identical blocks stays, and in each gap between them the i-th removed
 * block of the same kind pairs with the i-th added one as "changed". */
export function diffBlocks(older: readonly Block[], newer: readonly Block[]): DiffRow[] {
  const n = older.length;
  const m = newer.length;
  // Past this a plan is not being read block by block: all of it counts as changed.
  if (n * m > 4_000_000) return [...older.map((block) => ({ status: "removed" as const, block })), ...newer.map((block) => ({ status: "added" as const, block }))];
  const same = (a: Block, b: Block) => a.kind === b.kind && a.text === b.text;
  const w = m + 1;
  const len = new Uint32Array((n + 1) * w);
  for (let i = n - 1; i >= 0; i--)
    for (let j = m - 1; j >= 0; j--)
      len[i * w + j] = same(older[i]!, newer[j]!) ? len[(i + 1) * w + j + 1]! + 1 : Math.max(len[(i + 1) * w + j]!, len[i * w + j + 1]!);
  const rows: DiffRow[] = [];
  let gapOld: Block[] = [];
  let gapNew: Block[] = [];
  const flush = () => {
    const paired = new Map<Block, Block>();
    for (const o of gapOld) {
      const b = gapNew.find((x) => x.kind === o.kind && !paired.has(x));
      if (b) paired.set(b, o);
      else rows.push({ status: "removed", block: o });
    }
    for (const b of gapNew) {
      const before = paired.get(b);
      rows.push(before ? { status: "changed", block: b, before } : { status: "added", block: b });
    }
    gapOld = [];
    gapNew = [];
  };
  let i = 0;
  let j = 0;
  while (i < n && j < m) {
    if (same(older[i]!, newer[j]!)) {
      flush();
      rows.push({ status: "same", block: newer[j]! });
      i++;
      j++;
    } else if (len[(i + 1) * w + j]! >= len[i * w + j + 1]!) gapOld.push(older[i++]!);
    else gapNew.push(newer[j++]!);
  }
  gapOld.push(...older.slice(i));
  gapNew.push(...newer.slice(j));
  flush();
  return rows;
}

/** An agent's answer that ended a turn: the plan, if one was proposed. */
export interface Plan {
  id: string;
  text: string;
}

/** The plans proposed in a chat, oldest first: the last finished answer of
 * each turn that ended WHILE THE CHAT WAS IN PLAN MODE (what the plan-mode
 * prompt asks the agent to end with). An answer given in another mode is not
 * a plan: without this, the first plan of a chat that switched to plan mode
 * was shown as a revision of the last ordinary answer. Answers of a turn
 * still running are not plans yet.
 *
 * The mode is followed through the chat's own `approval_mode_changed` items.
 * Before the first of them the log does not say what the mode was: it was the
 * chat's mode now (`now`) when there is no such item at all, and is taken as
 * not plan otherwise (a chat created in plan mode and switched since loses
 * only the comparison with its very first plan). */
export function plansOf(
  items: readonly { type: string; message_id?: string; text?: string; done?: boolean; mode?: string }[],
  now: string = "plan",
): Plan[] {
  const out: Plan[] = [];
  let last: Plan | null = null;
  let plan = items.some((it) => it.type === "approval_mode_changed") ? false : now === "plan";
  for (const it of items) {
    if (it.type === "approval_mode_changed") plan = it.mode === "plan";
    else if (it.type === "assistant" && it.done && it.message_id && it.text?.trim()) last = { id: it.message_id, text: it.text };
    else if (it.type === "turn_end") {
      if (last && plan) out.push(last);
      last = null;
    }
  }
  return out;
}

// ── notes kept while the page lives (per chat; not stored anywhere) ──

const KEPT = new Map<string, Note[]>();
const MAX_CHATS = 50;
export const MAX_NOTES = 200;
export const MAX_NOTE_CHARS = 8000;

export function keptNotes(chat: string): Note[] {
  return KEPT.get(chat) ?? [];
}

export function keepNotes(chat: string, notes: Note[]): void {
  KEPT.delete(chat);
  if (notes.length) KEPT.set(chat, notes.slice(-MAX_NOTES));
  while (KEPT.size > MAX_CHATS) KEPT.delete(KEPT.keys().next().value!);
}

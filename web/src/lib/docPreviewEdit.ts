// Changing text in the side panel and saving a copy (X.60, T100): the parts of
// the panel's editing that are not drawing. Loaded only with the panel (a lazy
// chunk), never in the first load.
import type { PBlock, PCell, PreviewData, PreviewTarget, PRow, PRun } from "./docPreview";

/** What the server takes: the place (a paragraph's tag or a cell's address) and the text typed. */
export interface Change {
  at: string;
  text: string;
}

/** The route that saves a copy of a target, and its body (the sheet is the shown one's place). */
export function editRequest(t: PreviewTarget, sheet: number | undefined, changes: Change[]): { url: string; body: unknown } {
  const s = encodeURIComponent(t.session);
  const rest = { ...(sheet === undefined ? {} : { sheet }), changes };
  if (t.fileId !== undefined) return { url: `/api/sessions/${s}/files/${encodeURIComponent(t.fileId)}/edit`, body: rest };
  return { url: `/api/sessions/${s}/folder-edit`, body: { path: t.path, ...rest } };
}

/** The text a block or cell shows now: what a field starts with. */
export const textOf = (runs: PRun[]) => runs.map((r) => r.t).join("");

/** A paragraph's own number, from its tag (`p12#00a1`: paragraph 12 of the body). */
export function paragraphNumber(at: string): string {
  const m = /^p(\d{1,9})#/.exec(at);
  return m ? m[1]! : at.split("#")[0] ?? at;
}

/** Whether changing the paragraph drops formatting the preview shows: its runs differ
 * (the engine keeps the first run's properties, the other runs go). */
export const losesFormatting = (runs: PRun[]) => runs.length > 1;

/** How a path is written in the message so the composer reads it back as a mention
 * (`@path`, or `@"a path"` when it holds a space); none when it cannot be read back
 * (a quote, a line break, an `@`, an invisible or direction character). */
export function mentionOf(path: string): string | null {
  // Not a control, a line separator, an invisible or direction character
  // (category Cf: U+200B to U+200F, U+202A to U+202E, U+2060 to U+2069, U+FEFF), a
  // private-use character, a quote or an `@`: the path would read as something else.
  if (!path || /[\p{Cc}\p{Cf}\p{Co}\p{Zl}\p{Zp}"@]/u.test(path)) return null;
  return /\s/.test(path) ? `@"${path}"` : `@${path}`;
}

/** The text of the message with the mention added once, after what is there. */
export const withMention = (text: string, mention: string) => (text.includes(mention) ? text : `${text}${text && !/\s$/.test(text) ? " " : ""}${mention} `);

const colNum = (a: string) => [...a.replace(/\d+$/, "")].reduce((n, c) => n * 26 + c.charCodeAt(0) - 64, 0);
const rowNum = (a: string) => Number(/\d+$/.exec(a)?.[0] ?? 0);

/** The preview with the changes in it, for showing the copy: a paragraph keeps its first
 * run's formatting, a cell is what was typed. Display only; the copy itself is the server's. */
export function applyChanges(p: PreviewData, changes: Change[]): PreviewData {
  const by = new Map(changes.map((c) => [c.at, c.text]));
  if (p.kind === "doc") {
    const blocks = p.blocks.map((b): PBlock => {
      if (b.k === "t" || !b.at || !by.has(b.at)) return b;
      const first = b.r[0];
      return { ...b, at: undefined, r: [{ ...(first ? { b: first.b, i: first.i, s: first.s } : {}), t: by.get(b.at)! }] };
    });
    return { ...p, blocks };
  }
  const rows: PRow[] = p.rows.map((r) => ({ n: r.n, c: [...r.c] }));
  for (const [a, text] of by) {
    const n = rowNum(a);
    let row = rows.find((r) => r.n === n);
    if (!row) {
      row = { n, c: [] };
      rows.push(row);
    }
    const cell: PCell | null = text === "" ? null : text.startsWith("=") ? { a, f: text } : { a, v: text };
    row.c = row.c.filter((c) => c.a !== a);
    if (cell) row.c.push(cell);
    row.c.sort((x, y) => colNum(x.a) - colNum(y.a));
  }
  return { ...p, rows: rows.filter((r) => r.c.length).sort((x, y) => x.n - y.n) };
}

/** The places a refusal names, from the error's body: `message` per change in the order sent. */
export function refusals(err: unknown, sent: string[]): { places: Record<string, string>; sentence: string } {
  const b = (err as { body?: { results?: unknown } } | null)?.body;
  const places: Record<string, string> = {};
  if (b && Array.isArray(b.results)) {
    b.results.forEach((r: unknown, i: number) => {
      const x = r as { ok?: boolean; message?: unknown };
      const at = sent[i];
      if (at !== undefined && x && x.ok === false && typeof x.message === "string") places[at] = x.message;
    });
  }
  return { places, sentence: err instanceof Error ? err.message : String(err) };
}

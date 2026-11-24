// The /compact command in a chat's composer (ADOPTION_PLAN X.20). No
// imports: the composer is in the first load.

/** What a message starting with `/compact` asks for. */
export type CompactAsk = { focus: string } | { error: string };

/** The start of every reason a `compact_failed` carries (`compact_failed_reason` in Rust). */
export const COMPACT_FAILED_LEAD = "The chat was not summarised:";

/**
 * `/compact` and the focus text after it on the same line (`""`: none);
 * a refusal with a hint when more lines follow (the focus is one line);
 * `null` for any other text, which is a message.
 */
export function parseCompact(text: string): CompactAsk | null {
  const t = text.trim();
  const m = /^\/compact(?:[ \t]+([^\n]*))?(\n[\s\S]*)?$/.exec(t);
  if (!m) return null;
  if (m[2] != null && m[2].trim() !== "") {
    return { error: "/compact takes its focus on the same line: write it after /compact, on one line." };
  }
  return { focus: (m[1] ?? "").trim() };
}

/**
 * The `/` menu's query (X.65): the word after a leading `/` while the caret
 * is in that first word (`""` right after the `/`), else `null`. A path
 * (`/usr/bin`) is no command word, so it opens nothing.
 */
export function slashQuery(text: string, caret: number): string | null {
  const m = /^\/([\w.-]*)(?=\s|$)/.exec(text);
  return m && caret >= 1 && caret <= m[0].length ? m[1]! : null;
}

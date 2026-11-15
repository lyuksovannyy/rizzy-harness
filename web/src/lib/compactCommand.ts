// The /compact command in a chat's composer (ADOPTION_PLAN X.20). No
// imports: the composer is in the first load.

/** `/compact` and the focus text after it (`""`: none); `null` for any other text. */
export function parseCompact(text: string): string | null {
  const m = /^\/compact(?:\s+([\s\S]*))?$/.exec(text.trim());
  return m ? (m[1] ?? "").trim() : null;
}

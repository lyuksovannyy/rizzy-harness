// `@path` mentions (T63), the parts only a chat with a folder needs: the
// paths a message mentions and the folder's completion. Loaded on the first
// path token or send there, so the first load does not carry them.
import { pageApi } from "../apiPages";
import { isPathToken } from "./mentions";

/** The most paths one message mentions (the server's limit). */
export const MAX_FILE_MENTIONS = 64;

/** The paths `text` mentions: each `@token` at the start, after a space or
 * an opening bracket or quote, that names a file (`isPathToken`), without the punctuation that ends a
 * sentence after it, each once, at most `MAX_FILE_MENTIONS`. */
export function fileMentions(text: string): string[] {
  const out: string[] = [];
  for (const m of text.matchAll(/(^|[\s([{'"])@([^\s@]+)/g)) {
    let p = m[2]!.replace(/[,;:!?)\]}'"]+$/, "");
    if (p.length > 1 && p.endsWith(".") && !p.endsWith("..") && !p.endsWith("/.")) p = p.slice(0, -1);
    if (isPathToken(p) && !out.includes(p)) out.push(p);
    if (out.length >= MAX_FILE_MENTIONS) break;
  }
  return out;
}

/** Paths of chat `id`'s folder for what was typed after `@`. */
export async function listFiles(id: string, q: string): Promise<string[]> {
  return (await pageApi.files(id, q)).paths;
}

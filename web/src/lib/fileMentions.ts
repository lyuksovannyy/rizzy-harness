// `@path` mentions (T63), the parts only a chat with a folder needs: the
// paths a message mentions and the folder's completion. Loaded on the first
// path token or send there, so the first load does not carry them.
import { pageApi } from "../apiPages";
import { isPathToken } from "./mentions";

/** The most paths one message mentions (the server's limit). */
export const MAX_FILE_MENTIONS = 64;

/** The path tokens `text` holds: each `@token` at the start, after a space
 * or an opening bracket or quote, that looks like a path (`isPathToken`),
 * without the punctuation that ends a sentence after it, each once, at most
 * `MAX_FILE_MENTIONS`. Only those the folder holds become mentions
 * (`existing`). */
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

/** The query whose answer lists `token` itself when it exists: a folder
 * (`src/`) is found among its parent's names, a file among its folder's. */
function probe(token: string): string {
  return token.endsWith("/") ? token.slice(0, -1) : token;
}

/** Which of `tokens` name a file or folder of chat `id`'s folder, asked
 * one at a time (the server lets one person wait for a few lists at once),
 * the answers kept in `known` across calls. A token the folder does not
 * hold is the person's text, not a mention (`@types/node`). */
export async function existing(id: string, tokens: string[], known: Map<string, boolean>): Promise<string[]> {
  for (const tok of tokens) {
    if (known.has(tok)) continue;
    try {
      known.set(tok, (await listFiles(id, probe(tok))).includes(tok));
    } catch {
      // Not known now: asked again with the next change.
      return tokens.filter((x) => known.get(x));
    }
  }
  return tokens.filter((x) => known.get(x));
}

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

/** How long an answer about a path is trusted (a file may appear later). */
export const KNOWN_MS = 10_000;

/** What the folder said about a token, and when. */
export type Known = Map<string, { ok: boolean; at: number }>;

/** The question that asks for `token` itself: its folder's entries that
 * start with its name (`./` for the chat's folder), never a walk; the
 * node lists an exact name first. */
function probe(token: string): { q: string; want: string } {
  const t = token.endsWith("/") ? token.slice(0, -1) : token;
  return t.includes("/") ? { q: t, want: token } : { q: `./${t}`, want: `./${token}` };
}

/** Which of `tokens` name a file or folder of chat `id`'s folder, asked
 * one at a time (the server lets one person wait for a few lists at once),
 * answers kept in `known` for `KNOWN_MS`. A token the folder does not hold
 * is the person's text, not a mention (`@types/node`). */
export async function existing(id: string, tokens: string[], known: Known, now = Date.now()): Promise<string[]> {
  const fresh = (tok: string) => {
    const k = known.get(tok);
    return k && now - k.at < KNOWN_MS ? k : undefined;
  };
  for (const tok of tokens) {
    if (fresh(tok)) continue;
    const { q, want } = probe(tok);
    try {
      known.set(tok, { ok: (await listFiles(id, q)).includes(want), at: now });
    } catch {
      // Not known now: asked again with the next change.
      break;
    }
  }
  return tokens.filter((x) => fresh(x)?.ok);
}

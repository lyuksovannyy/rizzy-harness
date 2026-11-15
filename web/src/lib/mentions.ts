export interface AgentLike {
  id: string;
  name: string;
}

/** The `@query` token the caret is in, if any. With `paths` (a chat with
 * a folder), the token may hold a path's characters too. */
export function activeMention(text: string, caret: number, paths = false): { start: number; query: string } | null {
  const before = text.slice(0, caret);
  const m = (paths ? /(^|\s)@([^\s@]*)$/ : /(^|\s)@([a-z0-9-]*)$/).exec(before);
  if (!m) return null;
  return { start: before.length - m[2]!.length - 1, query: m[2]! };
}

/** A token names a file or folder once it holds `/` or `.`; otherwise it is
 * an agent's handle (handles are `[a-z0-9-]`, so they never hold either). A
 * file whose name is a bare word (`Makefile`, a folder `coder`) is reached
 * as `./Makefile` or `coder/`. */
export function isPathToken(q: string): boolean {
  return /[/.]/.test(q);
}

export function filterAgents<A extends AgentLike>(agents: A[], query: string, limit = 6): A[] {
  const q = query.toLowerCase();
  return agents
    .filter((a) => a.id.startsWith(q) || a.name.toLowerCase().includes(q))
    .slice(0, limit);
}

/** Replace the token `text[start..caret]` with `@id ` (a folder, ending in
 * `/`, without the space, so its entries can follow). */
export function insertMention(text: string, start: number, caret: number, id: string): { text: string; caret: number } {
  const ins = id.endsWith("/") ? `@${id}` : `@${id} `;
  const after = text.slice(caret).replace(id.endsWith("/") ? /^(?!)/ : /^ /, "");
  return { text: text.slice(0, start) + ins + after, caret: start + ins.length };
}

/** Same rule as the server: a leading `@id` naming a known agent, the
 * handle ending at a space, `,`, `:` or the end (so `@coder.md` is a file,
 * never the agent `coder`). */
export function leadingMention(text: string, knownIds: string[]): string | null {
  const m = /^\s*@([a-z0-9-]+)(?=$|[\s,:])/.exec(text);
  return m && knownIds.includes(m[1]!) ? m[1]! : null;
}

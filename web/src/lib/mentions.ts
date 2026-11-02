export interface AgentLike {
  id: string;
  name: string;
}

/** The `@query` token the caret is in, if any. */
export function activeMention(text: string, caret: number): { start: number; query: string } | null {
  const before = text.slice(0, caret);
  const m = /(^|\s)@([a-z0-9-]*)$/.exec(before);
  if (!m) return null;
  return { start: before.length - m[2]!.length - 1, query: m[2]! };
}

export function filterAgents<A extends AgentLike>(agents: A[], query: string, limit = 6): A[] {
  const q = query.toLowerCase();
  return agents
    .filter((a) => a.id.startsWith(q) || a.name.toLowerCase().includes(q))
    .slice(0, limit);
}

/** Replace the token `text[start..caret]` with `@id `. */
export function insertMention(text: string, start: number, caret: number, id: string): { text: string; caret: number } {
  const ins = `@${id} `;
  const after = text.slice(caret).replace(/^ /, "");
  return { text: text.slice(0, start) + ins + after, caret: start + ins.length };
}

/** Same rule as the server: a leading `@id` naming a known agent. */
export function leadingMention(text: string, knownIds: string[]): string | null {
  const m = /^\s*@([a-z0-9-]+)/.exec(text);
  return m && knownIds.includes(m[1]!) ? m[1]! : null;
}

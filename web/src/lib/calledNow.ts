// Which agents the text in the composer would call (X.37): loaded when the
// text holds an `@`, so the first screen carries none of it.
import { api } from "../api";
import type { Agent } from "../types";
import { MAX_RESOLVED, resolver, scanMentions, tagsOf } from "./mentionScan";

let login: Promise<string | undefined> | undefined;
/** The signed-in person's own login (a mention of it is the person, never an agent). */
export const loginName = () => (login ??= Promise.resolve().then(() => api.me()).then((m) => m.user.username, () => undefined));

/** What `text` calls and tags in the chat's agents `agents`: the ids called, in order (one more than the limit when it calls too many); the sub-agent-only ones it mentions; and whether it holds more resolved mentions than a message may. */
export async function calledIn(text: string, agents: Agent[]): Promise<{ called: string[]; tags: string[]; over: boolean }> {
  const m = scanMentions(text, resolver(agents, await loginName()));
  return { called: m.called, tags: tagsOf(m), over: m.found.length > MAX_RESOLVED };
}

// Which agents the text in the composer would call (X.37): loaded when the
// text holds an `@`, so the first screen carries none of it.
import { api } from "../api";
import type { Agent } from "../types";
import { resolver, scanMentions } from "./mentionScan";

let login: Promise<string | undefined> | undefined;
/** The signed-in person's own login (a mention of it is the person, never an agent). */
export const loginName = () => (login ??= Promise.resolve().then(() => api.me()).then((m) => m.user.username, () => undefined));

/** The ids `text` calls in the chat's agents `agents` (those that may answer), in order; one more than the limit when it calls too many. */
export async function calledIn(text: string, agents: Agent[]): Promise<string[]> {
  return scanMentions(text, resolver(agents, await loginName())).called;
}

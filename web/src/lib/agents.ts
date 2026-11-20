// The Agents page's hooks into the rest of the app (docs/agent-pages.md).
// Small on purpose: Shell and the composer load it with the first screen.
import type { Agent, Me } from "../types";

/**
 * Whether `me` may make, change and delete the server's agents: the
 * server's admins. A workspace's own agents (X.33) take its `settings`
 * permission; the listing says so (`may_edit` from `/api/agents`). The
 * server checks both (`require_agent_editor` in
 * crates/master/src/http/agent_edits.rs).
 */
export function mayEditAgents(me: Pick<Me, "role">): boolean {
  return me.role === "admin";
}

/** Agents the pickers and @mentions offer: those available as an agent, and `keep` (the one in use) in any case. */
export function pickable(agents: Agent[], keep?: string): Agent[] {
  return agents.filter((a) => a.features?.as_agent !== false || a.id === keep);
}

/** An agent that runs only as a sub-agent: it can be mentioned (a tag) but never answers a chat nor is called by a mention (X.37). */
export function isTag(a: Agent): boolean {
  return a.features?.as_agent === false && a.features?.as_subagent !== false;
}

/**
 * The server refuses a message in a chat whose agent runs only as a
 * sub-agent (`Resolver::top_level` in crates/master/src/resolver.rs:
 * `SubagentOnly`). The same fact, for a chat's reply box (X.41).
 */
export function takesNoChat(a: Agent | undefined): boolean {
  return a?.features?.as_agent === false;
}

/** Agents the `@` popup offers: the pickable ones and the sub-agent-only ones (marked, as tags). */
export function mentionable(agents: Agent[]): Agent[] {
  return agents.filter((a) => a.features?.as_agent !== false || isTag(a));
}

/**
 * A picker's hint for `a`: says first when it is the workspace's own agent
 * (written by that workspace's admins, X.33), then `rest`. From the server's
 * `scope`, never from the agent's own text.
 */
export function marked(a: Agent, rest?: string): string | undefined {
  return [a.scope === "workspace" && "workspace agent", rest].filter(Boolean).join(" · ") || undefined;
}

/** The id `#/agents/new` opens (lib/routes.ts): a new agent. Agents cannot take it. */
export const NEW_AGENT = "new";

// The Agents page's hooks into the rest of the app (docs/agent-pages.md).
// Small on purpose: Shell and the composer load it with the first screen.
import type { Agent, Me } from "../types";

/**
 * Whether `me` may make, change and delete agents. Today: the server's
 * admins, since agent edits are server-wide. Workspace agents would add
 * `can(me.workspace, "settings")` (lib/workspaces.ts) for the active
 * workspace's own. The server checks the same rule (`require_agent_editor`
 * in crates/master/src/http/agent_edits.rs).
 */
export function mayEditAgents(me: Pick<Me, "role">): boolean {
  return me.role === "admin";
}

/** Agents the pickers and @mentions offer: those available as an agent, and `keep` (the one in use) in any case. */
export function pickable(agents: Agent[], keep?: string): Agent[] {
  return agents.filter((a) => a.features?.as_agent !== false || a.id === keep);
}

/** The id `#/agents/new` opens (lib/routes.ts): a new agent. Agents cannot take it. */
export const NEW_AGENT = "new";

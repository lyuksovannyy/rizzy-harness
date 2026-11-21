// What an agent's page does with the agent's model chain (X.50): who may
// change it where, the chain the page starts from, and the two saves. The
// saves are the Settings pages' own routes: `PUT /api/admin/agent-models`
// with only this agent's entry, and `PATCH /api/workspaces/{id}` with the
// settings document read again just before, so another agent's chain or
// another setting changed meanwhile survives. Lazy: only the Agents page
// and the workspace's Agents panel load it.
import type { Scope } from "../api";
import { pageApi } from "../apiPages";
import type { Agent, AgentChain, WorkspaceSettings } from "../types";
import { type Chain, planProblems } from "./modelPlan";
import { dropped } from "./workspaceAdmin";

/** Where a chain is written: the server's, or one workspace's. */
export type ChainTarget = { at: "server" } | { at: "workspace"; id: string };

/** What the page does with the chain: edits it, or says who sets it. */
export type ChainPlace =
  | { edit: ChainTarget }
  | { read: "server" | "workspace" | "personal" | "personalOwn" };

/**
 * Who may change an agent's chain in the scope the page is opened in.
 * `editable` is what the page already knows: a server admin at the
 * server's scope, and in a workspace the listing's `may_edit` (the
 * `settings` permission, the same one `PATCH /api/workspaces/{id}` and the
 * agent edits ask for: `require_agent_editor` and `patch` in crates/master/src/http).
 * A Personal workspace has no settings of its own (that PATCH refuses it):
 * its chains are the server's, set by the server's admin, and an agent made
 * there has none (the server's route knows only the server's agents).
 */
export function chainPlace(o: {
  scope: Scope | null;
  editable: boolean;
  serverAdmin: boolean;
  personal: boolean;
  /** The agent is the scope's own (a workspace's), not the server's. */
  own: boolean;
}): ChainPlace {
  if (o.scope === "server") return o.editable ? { edit: { at: "server" } } : { read: "server" };
  if (o.scope === null) return { read: "server" };
  if (o.personal) {
    if (o.own) return { read: "personalOwn" };
    return o.serverAdmin ? { edit: { at: "server" } } : { read: "personal" };
  }
  return o.editable ? { edit: { at: "workspace", id: o.scope.workspace } } : { read: "workspace" };
}

export const NO_CHAIN: Chain = { model: null, fallbacks: [] };

export const chainOf = (c: AgentChain | null | undefined): Chain => (c ? { model: c.model, fallbacks: [...c.fallbacks] } : NO_CHAIN);

export const sameChain = (a: Chain, b: Chain) => a.model === b.model && a.fallbacks.length === b.fallbacks.length && a.fallbacks.every((f, i) => f === b.fallbacks[i]);

/** The server's chain of a listed agent. General without one runs on the
 * default model, which Settings shows as its main model, so it does here. An
 * agent without a chain lists General's fallbacks: not its own. */
export function serverChain(a: Agent): Chain {
  if (a.default_model) return { model: a.default_model, fallbacks: [...a.fallbacks] };
  if (a.id === "general" && a.main_model) return { model: a.main_model, fallbacks: [...a.fallbacks] };
  return NO_CHAIN;
}

/** Why the chain cannot be saved yet: Settings' own sentences. A workspace
 * chain may be none (it uses the server's), and so may any server agent's but
 * General's. */
export function chainProblems(c: Chain, agent: { id: string; name: string }, inherit: boolean, label: (id: string) => string): string[] {
  return planProblems({ [agent.id]: c }, [], [agent], label, inherit);
}

const asChain = (c: Chain): AgentChain | null => (c.model ? { model: c.model, fallbacks: [...c.fallbacks] } : null);

/** What a save answered: the chain the server keeps now, and what it
 * dropped as no longer known (workspace saves only; named as people read them). */
export interface Saved {
  chain: Chain;
  lost: string[];
}

/** Writes `agent`'s chain (none: it has none of its own). */
export async function saveChain(to: ChainTarget, agent: string, c: Chain, label: (id: string) => string = (x) => x): Promise<Saved> {
  const next = asChain(c);
  if (to.at === "server") {
    const state = await pageApi.saveAgentModels({ agent_models: { [agent]: next } });
    return { chain: chainOf(state?.agent_models?.[agent]), lost: [] };
  }
  // Read again just before: the document goes whole, so only this chain changes.
  const fresh = (await pageApi.workspace(to.id)).settings;
  const agent_models = { ...fresh.agent_models };
  if (next) agent_models[agent] = next;
  else delete agent_models[agent];
  const sent: WorkspaceSettings = { ...fresh, agent_models };
  const got = (await pageApi.updateWorkspace(to.id, { settings: sent })).settings;
  return { chain: chainOf(got.agent_models[agent]), lost: dropped(sent, got, { model: label }) };
}

/** The workspace's own chain for `agent`, as saved now (none: it has none). */
export async function workspaceChain(id: string, agent: string): Promise<Chain> {
  return chainOf((await pageApi.workspace(id)).settings.agent_models[agent]);
}

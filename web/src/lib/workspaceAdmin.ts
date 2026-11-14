// What the workspace settings panels work out without asking the server:
// what each fixed role may do, which choices the server's guards would refuse
// (so the panels turn them off; the server still checks every one), and the
// model chains a workspace saves. Settings only, so it loads with that lazy
// chunk; lib/workspaces.ts is part of the first load.
import type { AgentChain, Perm, WorkspaceRole, WorkspaceSettings } from "../types";
import type { Plan } from "./modelPlan";

export const ROLES: WorkspaceRole[] = ["owner", "admin", "member", "guest"];
/** The members panel's columns, in the server's wire order. */
export const PERMS: Perm[] = ["settings", "members", "routines"];

/** As fixed in the server (crates/master/src/workspaces.rs). Agent chats
 * are no role's: each member's level says (N.10a part 2). */
const ROLE_PERMS: Record<WorkspaceRole, Perm[]> = {
  owner: ["settings", "members", "routines"],
  admin: ["settings", "members", "routines"],
  member: ["routines"],
  guest: [],
};
const RANK: Record<WorkspaceRole, number> = { owner: 3, admin: 2, member: 1, guest: 0 };

/** What a role allows; a role this build does not know allows nothing. */
export const rolePerms = (r: WorkspaceRole): Perm[] => ROLE_PERMS[r] ?? [];

export const PERM_LABEL: Record<Perm, string> = {
  settings: "Change settings",
  members: "Manage members",
  routines: "Run routines",
};

const SUMMARY: Record<WorkspaceRole, string> = {
  owner: "You can do everything here, including deleting the workspace and making others owners.",
  admin: "You can run routines, change this workspace's settings and manage its members and their levels.",
  member: "You can chat and run routines here; your level says which agent chats you may start.",
  guest: "You can chat here, but not run routines; your level says which agent chats you may start.",
};

/** One sentence on what the viewer's role lets them do. */
export const roleSummary = (r: WorkspaceRole): string => SUMMARY[r] ?? "Your role here allows nothing yet.";

const outranks = (actor: WorkspaceRole, r: WorkspaceRole) =>
  rolePerms(actor).includes("members") && (r !== "owner" || actor === "owner") && RANK[r] <= RANK[actor];

/** Whether someone holding `actor` may give `role` to a member: only owners
 * make owners, and nobody gives a role above their own. */
export const canGive = (actor: WorkspaceRole, role: WorkspaceRole): boolean => outranks(actor, role);

/** Whether `actor` may change the role of, or remove, a member holding
 * `target`: never their own (leaving is in Overview), never an owner unless
 * they are one, never someone above them. */
export const canManage = (actor: WorkspaceRole, target: WorkspaceRole, self: boolean): boolean =>
  !self && outranks(actor, target);

/** The plan the Models panel starts from: the workspace's own chains, with
 * only the agents and models still listed. As the server reads them, a chain
 * whose main model is gone goes, and so does a fallback that is. */
export function workspacePlan(
  s: WorkspaceSettings,
  knownAgent: (id: string) => boolean,
  knownModel: (id: string) => boolean,
): Plan {
  const p: Plan = {};
  for (const [a, c] of Object.entries(s.agent_models)) {
    if (knownAgent(a) && knownModel(c.model)) p[a] = { model: c.model, fallbacks: c.fallbacks.filter(knownModel) };
  }
  return p;
}

/** A plan as `agent_models`: an agent without a main model is left out, so
 * it uses the server's chain. */
export function chainsOf(p: Plan): Record<string, AgentChain> {
  const out: Record<string, AgentChain> = {};
  for (const [a, c] of Object.entries(p)) if (c.model) out[a] = { model: c.model, fallbacks: [...c.fallbacks] };
  return out;
}

/** What `got` lacks of `sent`: the agents and models dropped as no longer
 * known (removed since they were saved), as people read them. */
export function dropped(
  sent: WorkspaceSettings,
  got: WorkspaceSettings,
  names: { agent?: (id: string) => string; model?: (id: string) => string } = {},
): string[] {
  const agent = names.agent ?? ((x) => x);
  const model = names.model ?? ((x) => x);
  const out: string[] = [];
  if (sent.default_agent && sent.default_agent !== got.default_agent) out.push(`the default agent ${agent(sent.default_agent)}`);
  for (const [a, c] of Object.entries(sent.agent_models)) {
    const g = got.agent_models[a];
    if (!g) out.push(`${agent(a)}'s chain`);
    else for (const m of [c.model, ...c.fallbacks]) if (m !== g.model && !g.fallbacks.includes(m)) out.push(model(m));
  }
  return [...new Set(out)];
}

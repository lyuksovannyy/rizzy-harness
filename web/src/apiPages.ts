// The API calls that only lazy pages make (Settings, the agent pages, Users,
// Memory, the workspace panels, the chat's own controls, the login form).
// They are split from api.ts so the first load does not carry them: that
// file keeps what the home screen, the sidebar and the composer call.
import { call, type Scope, scoped } from "./api";
import type {
  ExecLevel,
  Agent,
  AgentAvatar,
  AgentChain,
  AgentDetail,
  AgentFeatures,
  AdminWorkspace,
  AdminWorkspaceMember,
  Decision,
  FactKind,
  GraphifyStatus,
  MemoryCfg,
  MemoryFact,
  MemorySource,
  ModelListing,
  ModelTestResult,
  NewAgent,
  Prefs,
  Preset,
  ProviderKind,
  ProviderModel,
  ProvidersState,
  RemoteNode,
  Role,
  TokenSavings,
  User,
  WorkspaceDetail,
  WorkspaceMember,
  WorkspaceRole,
  WorkspaceSettings,
  WorkspaceSummary,
} from "./types";

const ws = (id: string) => `/api/workspaces/${encodeURIComponent(id)}`;
const adminWs = (id: string) => `/api/admin/workspaces/${encodeURIComponent(id)}`;

export const pageApi = {
  changePassword: (current: string, next: string) =>
    call<{ ok: boolean }>("POST", "/api/me/password", { current, new: next }),
  users: () => call<{ users: User[] }>("GET", "/api/users"),
  /** The answer carries the server-made temporary password, this once. */
  createUser: (b: { username: string; role: Role }) => call<User & { password: string }>("POST", "/api/users", b),
  resetPassword: (id: string) => call<User & { password: string }>("POST", `/api/users/${id}/password`),
  updateUser: (id: string, b: { role?: Role; disabled?: boolean }) => call<User>("PATCH", `/api/users/${id}`, b),
  /** An account's ceiling (N.10a part 2): server admins, never their own. */
  setCeiling: (id: string, level: ExecLevel) => call<User>("PUT", `/api/users/${id}/exec-ceiling`, { level }),
  deleteUser: (id: string) => call<{ ok: boolean }>("DELETE", `/api/users/${id}`),
  setupStatus: () => call<{ claimed: boolean }>("GET", "/api/setup"),
  setup: (claim_code: string, username: string, password: string) =>
    call<{ ok: boolean }>("POST", "/api/setup", { claim_code, username, password }),
  login: (username: string, password: string) =>
    call<{ ok: boolean }>("POST", "/api/login", { username, password }),
  remoteNodes: () =>
    call<{ nodes: RemoteNode[]; listener: { port: number; fingerprint: string } | null }>("GET", "/api/admin/nodes"),
  joinToken: () =>
    call<{ token: string; port: number; fingerprint: string; expires_in_s: number }>("POST", "/api/admin/nodes/join"),
  revokeNode: (id: string) => call<{ ok: boolean }>("DELETE", `/api/admin/nodes/${id}`),
  savings: () => call<TokenSavings>("GET", "/api/admin/savings"),
  saveSavings: (b: TokenSavings) => call<TokenSavings>("PUT", "/api/admin/savings", b),
  graphify: () => call<GraphifyStatus>("GET", "/api/admin/graphify"),
  selectVariant: (id: string, group: string, message_id: string) =>
    call<{ ok: boolean }>("POST", `/api/sessions/${id}/variants`, { group, message_id }),
  restoreTurn: (id: string, turn: string, force = false, rewind = false) =>
    call<{ ok: boolean }>("POST", `/api/sessions/${id}/turns/${turn}/restore`, { force, rewind }),
  /** The owner's new title for a chat (cleaned and capped by the server). */
  renameSession: (id: string, title: string) => call<{ ok: boolean; title: string }>("PUT", `/api/sessions/${id}/title`, { title }),
  /** `media` also deletes the caller's Media items that came from the chat. */
  deleteSession: (id: string, opts: { media?: boolean } = {}) =>
    call<{ ok: boolean }>("DELETE", `/api/sessions/${id}${opts.media ? "?media=1" : ""}`),
  setAgent: (id: string, agent_id: string) => call("POST", `/api/sessions/${id}/agent`, { agent_id }),
  saveAgentAvatar: (id: string, avatar: AgentAvatar) =>
    call<Agent>("PUT", `/api/admin/agents/${encodeURIComponent(id)}/avatar`, avatar),
  /** One agent with its instructions; the model line follows `scope` like `agents`. */
  agent: (id: string, scope?: Scope | null) =>
    call<AgentDetail>("GET", scoped(`/api/agents/${encodeURIComponent(id)}`, scope)),
  createAgent: (b: NewAgent, scope?: Scope | null) => call<AgentDetail>("POST", scoped("/api/agents", scope), b),
  /** Built-in and file agents take `summary` and `instructions` only. */
  updateAgent: (
    id: string,
    b: { name?: string; summary?: string; prompt?: string; instructions?: string; features?: AgentFeatures },
    scope?: Scope | null,
  ) => call<AgentDetail>("PATCH", scoped(`/api/agents/${encodeURIComponent(id)}`, scope), b),
  deleteAgent: (id: string) => call<{ ok: boolean }>("DELETE", `/api/agents/${encodeURIComponent(id)}`),
  /** A summary written by the default model from these instructions; nothing is saved. */
  agentSummary: (b: { name: string; prompt: string }) => call<{ summary: string }>("POST", "/api/agent-summary", b),
  providers: () => call<ProvidersState>("GET", "/api/admin/providers"),
  /** Always resolves; a failed connection is `{ok: false, error}`. */
  testProvider: (b: { preset: string; base_url?: string; api_key?: string; provider_id?: string }) =>
    call<{ ok: true; models: ProviderModel[] } | { ok: false; error: string }>("POST", "/api/admin/providers/test", b),
  saveProvider: (
    id: string,
    b: { name: string; preset: string; base_url?: string; api_key?: string; models: ProviderModel[] },
  ) => call<ProvidersState>("PUT", `/api/admin/providers/${encodeURIComponent(id)}`, b),
  deleteProvider: (id: string) => call<ProvidersState>("DELETE", `/api/admin/providers/${encodeURIComponent(id)}`),
  /** The server's cached listing; it lists once only if it has none. */
  providerModels: (id: string) => call<ModelListing>("GET", `/api/admin/providers/${encodeURIComponent(id)}/models`),
  refreshProviderModels: (id: string) =>
    call<ModelListing>("POST", `/api/admin/providers/${encodeURIComponent(id)}/models/refresh`),
  /** Drops a saved model (and its unavailable mark) from the provider and every agent. */
  removeProviderModel: (id: string, raw: string) =>
    call<ProvidersState>(
      "DELETE",
      `/api/admin/providers/${encodeURIComponent(id)}/models/${encodeURIComponent(raw)}`,
    ),
  /** Always resolves; one short request to `model` (raw), with the same key rules as testProvider. */
  testModel: (b: {
    model: string;
    provider_id?: string;
    preset?: string;
    base_url?: string;
    api_key?: string;
    kind?: ProviderKind;
  }) => call<ModelTestResult>("POST", "/api/admin/providers/models/test", b),
  saveAgentModels: (b: { agent_models: Record<string, AgentChain | null>; default_model?: string | null }) =>
    call<ProvidersState>("PUT", "/api/admin/agent-models", b),
  demoProvider: () => call<ProvidersState>("POST", "/api/admin/providers/demo"),
  saveSettings: (p: Prefs) => call<Prefs>("PUT", "/api/me/settings", p),
  addMemory: (text: string, kind: FactKind = "fact") =>
    call<MemoryFact>("POST", "/api/memory", { text, kind }),
  updateMemory: (id: string, b: { text?: string; status?: "active" }) =>
    call<MemoryFact>("PATCH", `/api/memory/${id}`, b),
  deleteMemory: (id: string) => call<{ ok: boolean }>("DELETE", `/api/memory/${id}`),
  memoryCfg: () => call<MemoryCfg>("GET", "/api/admin/memory"),
  saveMemoryCfg: (b: {
    recall: MemoryCfg["recall"];
    source: MemorySource;
    provider_id?: string;
    embeddings_url?: string;
    embeddings_model?: string;
    preset: Preset;
  }) =>
    call<MemoryCfg>("PUT", "/api/admin/memory", b),
  createWorkspace: (name: string) => call<WorkspaceSummary>("POST", "/api/workspaces", { name }),
  workspace: (id: string) => call<WorkspaceDetail>("GET", ws(id)),
  /** `settings` is the whole document. */
  updateWorkspace: (id: string, b: { name?: string; settings?: WorkspaceSettings }) =>
    call<WorkspaceDetail>("PATCH", ws(id), b),
  setWorkspaceMemory: (id: string, use_memory: boolean) =>
    call<WorkspaceDetail>("PUT", `${ws(id)}/memory`, { use_memory }),
  deleteWorkspace: (id: string) => call<{ ok: boolean }>("DELETE", ws(id)),
  workspaceMembers: (id: string) => call<{ members: WorkspaceMember[] }>("GET", `${ws(id)}/members`),
  addWorkspaceMember: (id: string, username: string, role: WorkspaceRole) =>
    call<WorkspaceMember>("POST", `${ws(id)}/members`, { username, role }),
  setWorkspaceMemberRole: (id: string, userId: string, role: WorkspaceRole) =>
    call<WorkspaceMember>("PATCH", `${ws(id)}/members/${encodeURIComponent(userId)}`, { role }),
  /** Your own user id: leave the workspace. */
  /** A member's level here: never one's own, never above one's own, `host` only by an owner. */
  setWorkspaceMemberLevel: (id: string, userId: string, level: ExecLevel) =>
    call<WorkspaceMember>("PUT", `${ws(id)}/members/${encodeURIComponent(userId)}/exec-level`, { level }),
  removeWorkspaceMember: (id: string, userId: string) =>
    call<{ ok: boolean }>("DELETE", `${ws(id)}/members/${encodeURIComponent(userId)}`),
  adminWorkspaces: () =>
    call<{ workspaces: AdminWorkspace[]; members_can_create: boolean }>("GET", "/api/admin/workspaces"),
  adminWorkspaceMembers: (id: string) => call<{ members: AdminWorkspaceMember[] }>("GET", `${adminWs(id)}/members`),
  saveWorkspacePolicy: (members_can_create: boolean) =>
    call<{ members_can_create: boolean }>("PUT", "/api/admin/workspaces/policy", { members_can_create }),
  /** Owner recovery: `owner` is the user id of an enabled member. */
  setWorkspaceOwner: (id: string, owner: string) => call<AdminWorkspace>("PATCH", adminWs(id), { owner }),
  adminDeleteWorkspace: (id: string) => call<{ ok: boolean }>("DELETE", adminWs(id)),
  cancel: (id: string) => call("POST", `/api/sessions/${id}/cancel`),
  approve: (id: string, approvalId: string, decision: Decision) =>
    call("POST", `/api/sessions/${id}/approvals/${approvalId}`, { decision }),
};

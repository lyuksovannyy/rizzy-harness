// The API calls that only lazy pages make (Settings, the agent pages, Users,
// Memory, the workspace panels, the chat's own controls, the login form).
// They are split from api.ts so the first load does not carry them: that
// file keeps what the home screen, the sidebar and the composer call.
import { call, HttpError, keyed, reach, type Scope, scoped, seen } from "./api";
import type {
  ApprovalMode,
  ExecLevel,
  Agent,
  AgentAvatar,
  AgentChain,
  AgentDetail,
  AgentFeatures,
  AdminWorkspace,
  AdminWorkspaceMember,
  Decision,
  TurnDiff,
  FactKind,
  GraphifyStatus,
  MemoryCallState,
  WorkspaceFacts,
  MemoryCfg,
  MemoryFact,
  MemorySource,
  ModelListing,
  ModelTestResult,
  NewAgent,
  Personalisation,
  Prefs,
  Preset,
  ProfileImages,
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
  /** Paths of a chat's folder for the composer's `@` completion (T63). */
  files: (id: string, q: string) => call<{ paths: string[] }>("GET", `/api/sessions/${id}/files?q=${encodeURIComponent(q)}`),
  /** What a turn changed (with `rewind`, it and every later turn), from its node. */
  turnDiff: (id: string, turn: string, rewind = false, signal?: AbortSignal) =>
    call<TurnDiff>("GET", `/api/sessions/${id}/turns/${encodeURIComponent(turn)}/diff${rewind ? "?rewind=1" : ""}`, undefined, signal),
  /** The owner's new title for a chat (cleaned and capped by the server). */
  renameSession: (id: string, title: string) => call<{ ok: boolean; title: string }>("PUT", `/api/sessions/${id}/title`, { title }),
  /** `media` also deletes the caller's Media items that came from the chat. */
  deleteSession: (id: string, opts: { media?: boolean } = {}) =>
    call<{ ok: boolean }>("DELETE", `/api/sessions/${id}${opts.media ? "?media=1" : ""}`),
  // Bounded: a pick that gets no answer frees the picker with a reason
  // (the server answers within its own timeouts; this is for a lost one).
  setAgent: (id: string, agent_id: string) =>
    call("POST", `/api/sessions/${id}/agent`, { agent_id }, AbortSignal.timeout(30_000)).catch((e) => {
      throw e instanceof DOMException && e.name === "TimeoutError" ? new Error("The server did not answer the agent change; try again.") : e;
    }),
  /** The node clamps it and records it; the chat shows the result (T50). */
  setApprovalMode: (id: string, mode: ApprovalMode) =>
    call<{ ok: boolean }>("POST", `/api/sessions/${id}/approval-mode`, { mode }),
  /** The node clamps it to 1..its limit and records it; the chat shows the result (X.14). */
  setStepLimit: (id: string, max_steps: number) =>
    call<{ ok: boolean }>("POST", `/api/sessions/${id}/step-limit`, { max_steps }),
  /** A person's Retry of the failed last turn (X.21): the node runs its message again. */
  retryTurn: (id: string, turn: string) =>
    call<{ ok: boolean }>("POST", `/api/sessions/${id}/turns/${encodeURIComponent(turn)}/retry`),
  /** A person's /compact (X.20): the node summarises the chat now and records the outcome in its log. */
  compact: (id: string, focus: string) =>
    call<{ ok: boolean }>("POST", `/api/sessions/${id}/compact`, focus ? { focus } : {}),
  saveAgentAvatar: (id: string, avatar: AgentAvatar) =>
    call<Agent>("PUT", `/api/admin/agents/${encodeURIComponent(id)}/avatar`, avatar),
  /** One agent with its instructions; the model line follows `scope` like `agents`. */
  agent: (id: string, scope?: Scope | null) =>
    call<AgentDetail>("GET", scoped(`/api/agents/${encodeURIComponent(id)}`, scope)),
  createAgent: (b: NewAgent, scope?: Scope | null) => call<AgentDetail>("POST", scoped("/api/agents", scope), b),
  /** Built-in and file agents take `summary` and `instructions` only. */
  updateAgent: (
    id: string,
    b: { name?: string; summary?: string; prompt?: string; instructions?: string; features?: AgentFeatures; delegate_to?: string[] | null },
    scope?: Scope | null,
  ) => call<AgentDetail>("PATCH", scoped(`/api/agents/${encodeURIComponent(id)}`, scope), b),
  /** Writes name their scope: a workspace's own agents, or the server's ("server"). */
  deleteAgent: (id: string, scope: Scope) => call<{ ok: boolean }>("DELETE", scoped(`/api/agents/${encodeURIComponent(id)}`, scope)),
  /** A summary written by the default model from these instructions; nothing is saved. */
  agentSummary: (b: { name: string; prompt: string }, scope: Scope) => call<{ summary: string }>("POST", scoped("/api/agent-summary", scope), b),
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
  savePricing: (currency: string) => call<{ currency: string }>("PUT", "/api/admin/pricing", { currency }),
  saveSettings: (p: Prefs) => call<Prefs>("PUT", "/api/me/settings", p),
  /** Personalisation alone; the rest stays as stored. */
  savePersonalisation: (p: Personalisation) => call<Prefs>("PUT", "/api/me/settings/personalisation", p),
  profileImages: () => call<ProfileImages>("GET", "/api/me/profile-images"),
  /** A picture's bytes (already made small in the browser): its new version. */
  setProfileImage: async (slot: "avatar" | "banner", file: Blob): Promise<ProfileImages> => {
    const res = await reach(fetch(`/api/me/${slot}`, { method: "PUT", credentials: "same-origin", headers: keyed({ "content-type": file.type }), body: file }));
    let data: unknown = null;
    try {
      data = await res.json();
    } catch {
      /* non-JSON */
    }
    seen(res.status, data);
    if (!res.ok) throw new HttpError(res.status, (data as { error?: string } | null)?.error ?? `${res.status} ${res.statusText}`);
    return data as ProfileImages;
  },
  removeProfileImage: (slot: "avatar" | "banner") => call<ProfileImages>("DELETE", `/api/me/${slot}`),
  addMemory: (text: string, kind: FactKind = "fact") =>
    call<MemoryFact>("POST", "/api/memory", { text, kind }),
  updateMemory: (
    id: string,
    b: { text?: string; status?: "active" | "retired"; place?: "user" | "workspace"; workspace_id?: string },
  ) => call<MemoryFact>("PATCH", `/api/memory/${id}`, b),
  memoryCall: (sessionId: string, callId: string) =>
    call<MemoryCallState>("GET", `/api/sessions/${sessionId}/memory/${encodeURIComponent(callId)}`),
  keepMemoryCall: (sessionId: string, callId: string) =>
    call<MemoryCallState>("POST", `/api/sessions/${sessionId}/memory/${encodeURIComponent(callId)}/keep`),
  dismissMemoryCall: (sessionId: string, callId: string) =>
    call<MemoryCallState>("POST", `/api/sessions/${sessionId}/memory/${encodeURIComponent(callId)}/dismiss`),
  undoMemoryCall: (sessionId: string, callId: string) =>
    call<MemoryCallState>("POST", `/api/sessions/${sessionId}/memory/${encodeURIComponent(callId)}/undo`),
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
  workspaceFacts: (id: string) => call<WorkspaceFacts>("GET", `${ws(id)}/facts`),
  addWorkspaceFact: (id: string, text: string) => call<MemoryFact>("POST", `${ws(id)}/facts`, { text }),
  updateWorkspaceFact: (id: string, fact: string, b: { text?: string; status?: "active" | "retired" }) =>
    call<MemoryFact>("PATCH", `${ws(id)}/facts/${fact}`, b),
  deleteWorkspaceFact: (id: string, fact: string) => call<{ ok: boolean }>("DELETE", `${ws(id)}/facts/${fact}`),
  setWorkspaceFactsSettings: (id: string, members_edit: boolean) =>
    call<{ members_edit: boolean }>("PUT", `${ws(id)}/facts/settings`, { members_edit }),
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
  /** `note` goes with a denial only (the server drops it with an allow). */
  approve: (id: string, approvalId: string, decision: Decision, note?: string) =>
    call("POST", `/api/sessions/${id}/approvals/${approvalId}`, note ? { decision, note } : { decision }),
  /** Remove a call allowed for the rest of the chat (T51). */
  revokeAllow: (id: string, grantId: string) => call("DELETE", `/api/sessions/${id}/allows/${grantId}`),
};

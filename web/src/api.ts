import type {
  Agent,
  AgentAvatar,
  AgentDetail,
  AgentFeatures,
  NewAgent,
  Notice,
  AttentionResponse,
  Decision,
  Features,
  TokenSavings,
  GraphifyStatus,
  Effort,
  FactKind,
  Me,
  MemoryCfg,
  MemoryFact,
  Preset,
  ModelInfo,
  NodeInfo,
  Prefs,
  Profile,
  MemorySource,
  AgentChain,
  ModelListing,
  ModelTestResult,
  ProviderKind,
  ProviderModel,
  ProvidersState,
  RawEvent,
  RemoteNode,
  FormReply,
  Role,
  SearchKind,
  SearchResponse,
  SessionRow,
  UploadedAttachment,
  User,
  AdminWorkspace,
  AdminWorkspaceMember,
  WorkspaceDetail,
  WorkspaceList,
  WorkspaceMember,
  WorkspaceRole,
  WorkspaceSettings,
  WorkspaceSummary,
} from "./types";

export class HttpError extends Error {
  constructor(
    public status: number,
    message: string,
  ) {
    super(message);
  }
}

export async function call<T>(method: string, path: string, body?: unknown, signal?: AbortSignal): Promise<T> {
  const res = await fetch(path, {
    method,
    signal,
    credentials: "same-origin",
    headers: body === undefined ? {} : { "content-type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  let data: unknown = null;
  try {
    data = await res.json();
  } catch {
    /* non-JSON */
  }
  if (!res.ok) {
    const msg = (data as { error?: string } | null)?.error ?? `${res.status} ${res.statusText}`;
    throw new HttpError(res.status, msg);
  }
  return data as T;
}

/** Whose settings a list follows: a workspace's (by id), or the server's own
 * (`"server"`, for the Server settings). Left out, the server uses the
 * caller's active workspace; the web always says which. */
export type Scope = { workspace: string } | "server";

function scoped(path: string, scope?: Scope | null): string {
  if (!scope) return path;
  const q = scope === "server" ? "scope=server" : `workspace=${encodeURIComponent(scope.workspace)}`;
  return `${path}${path.includes("?") ? "&" : "?"}${q}`;
}

const ws = (id: string) => `/api/workspaces/${encodeURIComponent(id)}`;
const adminWs = (id: string) => `/api/admin/workspaces/${encodeURIComponent(id)}`;

export const api = {
  /** The signed-in principal; the active workspace comes beside it (null
   * for the API token). */
  me: () => call<{ ok: boolean; app: string; user: Me; workspace: WorkspaceSummary | null }>("GET", "/api/me"),
  changePassword: (current: string, next: string) =>
    call<{ ok: boolean }>("POST", "/api/me/password", { current, new: next }),
  users: () => call<{ users: User[] }>("GET", "/api/users"),
  createUser: (b: { username: string; password: string; role: Role; can_agentic: boolean }) =>
    call<User>("POST", "/api/users", b),
  updateUser: (
    id: string,
    b: { role?: Role; disabled?: boolean; can_agentic?: boolean; password?: string },
  ) => call<User>("PATCH", `/api/users/${id}`, b),
  deleteUser: (id: string) => call<{ ok: boolean }>("DELETE", `/api/users/${id}`),
  setupStatus: () => call<{ claimed: boolean }>("GET", "/api/setup"),
  setup: (claim_code: string, username: string, password: string) =>
    call<{ ok: boolean }>("POST", "/api/setup", { claim_code, username, password }),
  login: (username: string, password: string) =>
    call<{ ok: boolean }>("POST", "/api/login", { username, password }),
  logout: () => call<{ ok: boolean }>("POST", "/api/logout"),
  nodes: () => call<{ nodes: NodeInfo[] }>("GET", "/api/nodes"),
  remoteNodes: () =>
    call<{ nodes: RemoteNode[]; listener: { port: number; fingerprint: string } | null }>("GET", "/api/admin/nodes"),
  joinToken: () =>
    call<{ token: string; port: number; fingerprint: string; expires_in_s: number }>("POST", "/api/admin/nodes/join"),
  revokeNode: (id: string) => call<{ ok: boolean }>("DELETE", `/api/admin/nodes/${id}`),
  /** The chats in a workspace (the API token: all, unless one is named).
   * `routineRuns` also lists the routine run chats left out once seen. */
  sessions: (workspace?: string | null, opts: { routineRuns?: boolean } = {}) =>
    call<{ sessions: SessionRow[] }>(
      "GET",
      scoped(`/api/sessions${opts.routineRuns ? "?routine_runs=1" : ""}`, workspace ? { workspace } : null),
    ),
  /** One chat, in whichever workspace it is. */
  session: (id: string) => call<SessionRow>("GET", `/api/sessions/${id}`),
  events: (id: string, after = 0) =>
    call<{ events: RawEvent[] }>("GET", `/api/sessions/${id}/events?after=${after}`),
  createSession: (b: {
    profile: Profile;
    cwd?: string;
    title?: string;
    node_id?: string;
    agent_id?: string;
    model?: string;
    incognito?: boolean;
    workspace_id?: string;
    project_id?: string;
  }) => call<{ session_id: string }>("POST", "/api/sessions", b),
  attention: () => call<AttentionResponse>("GET", "/api/attention"),
  savings: () => call<TokenSavings>("GET", "/api/admin/savings"),
  saveSavings: (b: TokenSavings) => call<TokenSavings>("PUT", "/api/admin/savings", b),
  graphify: () => call<GraphifyStatus>("GET", "/api/admin/graphify"),
  markSeen: (id: string, seq: number) => call<{ ok: boolean }>("POST", `/api/sessions/${id}/seen`, { seq }),
  /** What the viewer may use in the workspace (the sidebar's areas). */
  features: (scope?: Scope | null) => call<Features>("GET", scoped("/api/features", scope)),
  transcribe: async (audio: Blob): Promise<string> => {
    const res = await fetch("/api/transcribe", {
      method: "POST",
      credentials: "same-origin",
      headers: { "content-type": audio.type || "audio/webm" },
      body: audio,
    });
    const data = (await res.json().catch(() => null)) as { text?: string; error?: string } | null;
    if (!res.ok) throw new HttpError(res.status, data?.error ?? `${res.status} ${res.statusText}`);
    return data?.text ?? "";
  },
  selectVariant: (id: string, group: string, message_id: string) =>
    call<{ ok: boolean }>("POST", `/api/sessions/${id}/variants`, { group, message_id }),
  restoreTurn: (id: string, turn: string, force = false, rewind = false) =>
    call<{ ok: boolean }>("POST", `/api/sessions/${id}/turns/${turn}/restore`, { force, rewind }),
  /** `media` also deletes the caller's Media items that came from the chat. */
  deleteSession: (id: string, opts: { media?: boolean } = {}) =>
    call<{ ok: boolean }>("DELETE", `/api/sessions/${id}${opts.media ? "?media=1" : ""}`),
  prompt: (
    id: string,
    text: string,
    opts: {
      agent_id?: string;
      effort?: Effort;
      attachments?: string[];
      compare?: string[];
      reply?: FormReply;
      /** Media item ids. */
      images?: string[];
    } = {},
  ) => call("POST", `/api/sessions/${id}/prompt`, { text, ...opts }),
  upload: async (file: File): Promise<UploadedAttachment> => {
    const res = await fetch(`/api/attachments?name=${encodeURIComponent(file.name)}`, {
      method: "POST",
      credentials: "same-origin",
      headers: { "content-type": "application/octet-stream" },
      body: file,
    });
    const data = (await res.json().catch(() => null)) as UploadedAttachment & { error?: string };
    if (!res.ok) throw new HttpError(res.status, data?.error ?? `${res.status} ${res.statusText}`);
    return data;
  },
  setAgent: (id: string, agent_id: string) => call("POST", `/api/sessions/${id}/agent`, { agent_id }),
  agents: (scope?: Scope | null) => call<{ agents: Agent[]; default: string }>("GET", scoped("/api/agents", scope)),
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
  notices: (scope?: Scope | null) => call<{ notices: Notice[] }>("GET", scoped("/api/notices", scope)),
  models: (scope?: Scope | null) => call<{ models: ModelInfo[] }>("GET", scoped("/api/models", scope)),
  /** One page of the search panel (`in`: a tab, or "all"), in `workspace`;
   * `suggest` asks for the bar's similar chats instead. */
  search: (
    q: string,
    opts: {
      in?: "all" | SearchKind;
      cursor?: string | null;
      suggest?: boolean;
      workspace?: string | null;
      signal?: AbortSignal;
    } = {},
  ) => {
    const p = new URLSearchParams({ q });
    if (opts.suggest) p.set("suggest", "1");
    else p.set("in", opts.in ?? "chats");
    if (opts.cursor) p.set("cursor", opts.cursor);
    return call<SearchResponse>(
      "GET",
      scoped(`/api/search?${p}`, opts.workspace ? { workspace: opts.workspace } : null),
      undefined,
      opts.signal,
    );
  },
  classifier: () => call<unknown>("GET", "/api/me/classifier"),
  saveClassifier: (state: unknown) => call<{ ok: boolean }>("PUT", "/api/me/classifier", state),
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
  settings: () => call<Prefs>("GET", "/api/me/settings"),
  saveSettings: (p: Prefs) => call<Prefs>("PUT", "/api/me/settings", p),
  memories: () => call<{ memories: MemoryFact[]; backend: string }>("GET", "/api/memory"),
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
  workspaces: () => call<WorkspaceList>("GET", "/api/workspaces"),
  createWorkspace: (name: string) => call<WorkspaceSummary>("POST", "/api/workspaces", { name }),
  /** Makes it the active workspace (lists, defaults and new chats follow it). */
  switchWorkspace: (workspace_id: string) => call<{ active: string }>("PUT", "/api/me/workspace", { workspace_id }),
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

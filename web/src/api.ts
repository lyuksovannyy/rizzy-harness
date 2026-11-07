import type {
  Agent,
  AgentAvatar,
  Notice,
  AttentionItem,
  Decision,
  NodeAttention,
  SetupCommands,
  SetupRecord,
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
  ApiStyle,
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
  SetupState,
  SearchHit,
  SessionRow,
  UploadedAttachment,
  User,
} from "./types";

export class HttpError extends Error {
  constructor(
    public status: number,
    message: string,
  ) {
    super(message);
  }
}

async function call<T>(method: string, path: string, body?: unknown, signal?: AbortSignal): Promise<T> {
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

export const api = {
  me: () => call<{ ok: boolean; app: string; user: Me }>("GET", "/api/me"),
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
  probeHost: (host: string, port: number) =>
    call<{ fingerprint: string }>("POST", "/api/admin/provision/probe", { host, port }),
  startSetup: (b: Record<string, unknown>) => call<{ id: string }>("POST", "/api/admin/provision", b),
  setupState: (id: string) => call<SetupState>("GET", `/api/admin/provision/${id}`),
  setupAnswer: (id: string, step: string, choice: string) =>
    call<{ ok: boolean }>("POST", `/api/admin/provision/${id}/answer`, { step, choice }),
  setupRecheck: (id: string) => call<SetupState>("POST", `/api/admin/provision/${id}/recheck`),
  revokeNode: (id: string) => call<{ ok: boolean }>("DELETE", `/api/admin/nodes/${id}`),
  sessions: () => call<{ sessions: SessionRow[] }>("GET", "/api/sessions"),
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
  }) => call<{ session_id: string }>("POST", "/api/sessions", b),
  attention: () => call<{ items: AttentionItem[]; nodes?: NodeAttention[] }>("GET", "/api/attention"),
  setups: () => call<{ setups: { setup: SetupRecord; commands: SetupCommands }[] }>("GET", "/api/admin/node-setups"),
  updateSetup: (id: string, b: { mode?: "auto" | "manual"; watch_ssh?: boolean; watch_firewall?: boolean }) =>
    call<SetupRecord>("PATCH", `/api/admin/node-setups/${id}`, b),
  forgetSetup: (id: string) => call<{ ok: boolean }>("DELETE", `/api/admin/node-setups/${id}`),
  checkSetup: (id: string) => call<SetupRecord>("POST", `/api/admin/node-setups/${id}/check`),
  setFirewall: (id: string, b: Record<string, unknown>) => call<SetupRecord>("POST", `/api/admin/node-setups/${id}/firewall`, b),
  savings: () => call<TokenSavings>("GET", "/api/admin/savings"),
  saveSavings: (b: TokenSavings) => call<TokenSavings>("PUT", "/api/admin/savings", b),
  graphify: () => call<GraphifyStatus>("GET", "/api/admin/graphify"),
  allowGraphify: (allowed: boolean) => call<GraphifyStatus>("PUT", "/api/admin/graphify", { allowed }),
  installGraphify: () => call<GraphifyStatus>("POST", "/api/admin/graphify/install"),
  markSeen: (id: string, seq: number) => call<{ ok: boolean }>("POST", `/api/sessions/${id}/seen`, { seq }),
  features: () => call<{ voice: boolean; memory: boolean }>("GET", "/api/features"),
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
  deleteSession: (id: string) => call<{ ok: boolean }>("DELETE", `/api/sessions/${id}`),
  prompt: (
    id: string,
    text: string,
    opts: { agent_id?: string; effort?: Effort; attachments?: string[]; compare?: string[]; reply?: FormReply } = {},
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
  agents: () => call<{ agents: Agent[]; default: string }>("GET", "/api/agents"),
  saveAgentAvatar: (id: string, avatar: AgentAvatar) =>
    call<Agent>("PUT", `/api/admin/agents/${encodeURIComponent(id)}/avatar`, avatar),
  notices: () => call<{ notices: Notice[] }>("GET", "/api/notices"),
  models: () => call<{ models: ModelInfo[] }>("GET", "/api/models"),
  search: (q: string, suggest = false, signal?: AbortSignal) =>
    call<{ results: SearchHit[] }>(
      "GET",
      `/api/search?q=${encodeURIComponent(q)}${suggest ? "&suggest=true" : ""}`,
      undefined,
      signal,
    ),
  classifier: () => call<unknown>("GET", "/api/me/classifier"),
  saveClassifier: (state: unknown) => call<{ ok: boolean }>("PUT", "/api/me/classifier", state),
  providers: () => call<ProvidersState>("GET", "/api/admin/providers"),
  /** Always resolves; a failed connection is `{ok: false, error}`. */
  /** `memory`: reuse the embeddings key saved in [memory] when the URL matches. */
  testProvider: (b: { preset: string; base_url?: string; api_key?: string; provider_id?: string; memory?: boolean }) =>
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
    memory?: boolean;
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
    api_style?: ApiStyle;
    embeddings_key?: string;
    preset: Preset;
  }) =>
    call<MemoryCfg>("PUT", "/api/admin/memory", b),
  cancel: (id: string) => call("POST", `/api/sessions/${id}/cancel`),
  approve: (id: string, approvalId: string, decision: Decision) =>
    call("POST", `/api/sessions/${id}/approvals/${approvalId}`, { decision }),
};

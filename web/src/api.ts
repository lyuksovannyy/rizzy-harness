import type {
  Agent,
  Decision,
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
  ProviderKind,
  ProviderModel,
  ProviderState,
  RawEvent,
  RemoteNode,
  Role,
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
  restoreTurn: (id: string, turn: string, force = false) =>
    call<{ ok: boolean }>("POST", `/api/sessions/${id}/turns/${turn}/restore`, { force }),
  deleteSession: (id: string) => call<{ ok: boolean }>("DELETE", `/api/sessions/${id}`),
  prompt: (
    id: string,
    text: string,
    opts: { agent_id?: string; effort?: Effort; attachments?: string[] } = {},
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
  provider: () => call<ProviderState>("GET", "/api/admin/provider"),
  testProvider: (b: { kind: ProviderKind; base_url?: string; api_key?: string }) =>
    call<{ ok: boolean; models?: string[]; error?: string }>("POST", "/api/admin/provider/test", b),
  saveProvider: (b: {
    kind: ProviderKind;
    base_url?: string;
    api_key?: string;
    model?: string;
    models: ProviderModel[];
  }) => call<ProviderState>("PUT", "/api/admin/provider", b),
  settings: () => call<Prefs>("GET", "/api/me/settings"),
  saveSettings: (p: Prefs) => call<Prefs>("PUT", "/api/me/settings", p),
  memories: () => call<{ memories: MemoryFact[]; backend: string }>("GET", "/api/memory"),
  addMemory: (text: string, kind: FactKind = "fact") =>
    call<MemoryFact>("POST", "/api/memory", { text, kind }),
  updateMemory: (id: string, b: { text?: string; status?: "active" }) =>
    call<MemoryFact>("PATCH", `/api/memory/${id}`, b),
  deleteMemory: (id: string) => call<{ ok: boolean }>("DELETE", `/api/memory/${id}`),
  memoryCfg: () => call<MemoryCfg>("GET", "/api/admin/memory"),
  saveMemoryCfg: (b: { embeddings_url: string; embeddings_model: string; preset: Preset }) =>
    call<MemoryCfg>("PUT", "/api/admin/memory", b),
  cancel: (id: string) => call("POST", `/api/sessions/${id}/cancel`),
  approve: (id: string, approvalId: string, decision: Decision) =>
    call("POST", `/api/sessions/${id}/approvals/${approvalId}`, { decision }),
};

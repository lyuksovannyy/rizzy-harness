import type {
  Agent,
  Decision,
  Effort,
  Me,
  ModelInfo,
  NodeInfo,
  Prefs,
  Profile,
  ProviderKind,
  ProviderModel,
  ProviderState,
  RawEvent,
  Role,
  SearchHit,
  SessionRow,
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
  }) => call<{ session_id: string }>("POST", "/api/sessions", b),
  prompt: (id: string, text: string, opts: { agent_id?: string; effort?: Effort } = {}) =>
    call("POST", `/api/sessions/${id}/prompt`, { text, ...opts }),
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
  cancel: (id: string) => call("POST", `/api/sessions/${id}/cancel`),
  approve: (id: string, approvalId: string, decision: Decision) =>
    call("POST", `/api/sessions/${id}/approvals/${approvalId}`, { decision }),
};

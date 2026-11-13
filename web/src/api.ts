import type {
  Agent,
  Notice,
  AttentionResponse,
  Features,
  Effort,
  Me,
  MemoryFact,
  ModelInfo,
  NodeInfo,
  Prefs,
  Profile,
  ExecMode,
  RawEvent,
  FormReply,
  SearchKind,
  SearchResponse,
  SessionRow,
  UploadedAttachment,
  WorkspaceList,
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

// The login key (Gate 3 P-1). Browsers send the login cookie to every port
// of the host, so another local server (the agent's dev server) gets it
// too; the cookie alone authenticates nothing. The key comes in the answer
// that made the login and goes out as a header on every request and as the
// socket's first message (live.ts). It lives in localStorage: per origin,
// port included, so no other port's page or server ever sees it, and a new
// tab of this origin has it. Never in a URL; cleared on any sign-out.
const KEY = "harness.login";

export function loginKey(): string | null {
  try {
    return localStorage.getItem(KEY);
  } catch {
    return null;
  }
}

export function forgetLogin() {
  try {
    localStorage.removeItem(KEY);
  } catch {
    /* storage gone */
  }
}

/** `h` with the login key, for requests made outside `call`. */
export const keyed = (h: Record<string, string> = {}): Record<string, string> => {
  const k = loginKey();
  return k ? { ...h, "x-harness-login": k } : h;
};

/** Called on any 401: the login ended, or this tab has no key for it. */
export const login = { ended: () => {} };

/** Checks an answer outside `call`: a 401 signs out; a new login's key is kept. */
export function seen(status: number, data: unknown) {
  const k = (data as { login_key?: string } | null)?.login_key;
  if (k)
    try {
      localStorage.setItem(KEY, k);
    } catch {
      /* not kept: the next request signs out */
    }
  if (status === 401) {
    forgetLogin();
    login.ended();
  }
}

export async function call<T>(method: string, path: string, body?: unknown, signal?: AbortSignal): Promise<T> {
  const res = await fetch(path, {
    method,
    signal,
    credentials: "same-origin",
    headers: keyed(body === undefined ? {} : { "content-type": "application/json" }),
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  let data: unknown = null;
  try {
    data = await res.json();
  } catch {
    /* non-JSON */
  }
  seen(res.status, data);
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

export function scoped(path: string, scope?: Scope | null): string {
  if (!scope) return path;
  const q = scope === "server" ? "scope=server" : `workspace=${encodeURIComponent(scope.workspace)}`;
  return `${path}${path.includes("?") ? "&" : "?"}${q}`;
}


export const api = {
  /** The signed-in principal; the active workspace comes beside it (null
   * for the API token). */
  me: () => call<{ ok: boolean; app: string; user: Me; workspace: WorkspaceSummary | null }>("GET", "/api/me"),
  logout: () => call<{ ok: boolean }>("POST", "/api/logout"),
  nodes: () => call<{ nodes: NodeInfo[] }>("GET", "/api/nodes"),
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
    exec_mode?: ExecMode;
  }) => call<{ session_id: string }>("POST", "/api/sessions", b),
  attention: () => call<AttentionResponse>("GET", "/api/attention"),
  markSeen: (id: string, seq: number) => call<{ ok: boolean }>("POST", `/api/sessions/${id}/seen`, { seq }),
  /** What the viewer may use in the workspace (the sidebar's areas). */
  features: (scope?: Scope | null) => call<Features>("GET", scoped("/api/features", scope)),
  transcribe: async (audio: Blob): Promise<string> => {
    const res = await fetch("/api/transcribe", {
      method: "POST",
      credentials: "same-origin",
      headers: keyed({ "content-type": audio.type || "audio/webm" }),
      body: audio,
    });
    const data = (await res.json().catch(() => null)) as { text?: string; error?: string } | null;
    seen(res.status, data);
    if (!res.ok) throw new HttpError(res.status, data?.error ?? `${res.status} ${res.statusText}`);
    return data?.text ?? "";
  },
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
      headers: keyed({ "content-type": "application/octet-stream" }),
      body: file,
    });
    const data = (await res.json().catch(() => null)) as UploadedAttachment & { error?: string };
    seen(res.status, data);
    if (!res.ok) throw new HttpError(res.status, data?.error ?? `${res.status} ${res.statusText}`);
    return data;
  },
  agents: (scope?: Scope | null) => call<{ agents: Agent[]; default: string }>("GET", scoped("/api/agents", scope)),
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
  settings: () => call<Prefs>("GET", "/api/me/settings"),
  memories: () => call<{ memories: MemoryFact[]; backend: string }>("GET", "/api/memory"),
  workspaces: () => call<WorkspaceList>("GET", "/api/workspaces"),
  /** Makes it the active workspace (lists, defaults and new chats follow it). */
  switchWorkspace: (workspace_id: string) => call<{ active: string }>("PUT", "/api/me/workspace", { workspace_id }),
};

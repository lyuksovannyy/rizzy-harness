import type { Decision, NodeInfo, RawEvent, SessionRow } from "./types";

export class HttpError extends Error {
  constructor(
    public status: number,
    message: string,
  ) {
    super(message);
  }
}

async function call<T>(method: string, path: string, body?: unknown): Promise<T> {
  const res = await fetch(path, {
    method,
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
  me: () =>
    call<{ ok: boolean; app: string; user: { username: string; role: string } }>("GET", "/api/me"),
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
  createSession: (b: { cwd: string; title?: string; node_id?: string }) =>
    call<{ session_id: string }>("POST", "/api/sessions", b),
  prompt: (id: string, text: string) => call("POST", `/api/sessions/${id}/prompt`, { text }),
  cancel: (id: string) => call("POST", `/api/sessions/${id}/cancel`),
  approve: (id: string, approvalId: string, decision: Decision) =>
    call("POST", `/api/sessions/${id}/approvals/${approvalId}`, { decision }),
};

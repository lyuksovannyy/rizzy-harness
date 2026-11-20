// @vitest-environment happy-dom
// The login key (Gate 3 P-1): kept from the answer that made the login, sent
// on every request, forgotten on a 401 (which signs out). A new tab of this
// origin finds it in localStorage; no other port ever can.
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { api, call, forgetLogin, keyed, login, loginKey } from "./api";
import { pageApi } from "./apiPages";

const KEY = "a".repeat(64);
let answers: { status: number; body: unknown }[] = [];
const fetchFn = vi.fn(async (_path: string, _init?: RequestInit) => {
  const a = answers.shift() ?? { status: 200, body: { ok: true } };
  return new Response(JSON.stringify(a.body), { status: a.status });
});
const sent = (i: number) => new Headers(fetchFn.mock.calls[i]![1]!.headers).get("x-harness-login");

beforeEach(() => {
  answers = [];
  fetchFn.mockClear();
  vi.stubGlobal("fetch", fetchFn);
  localStorage.clear();
  login.ended = () => {};
});
afterEach(() => vi.unstubAllGlobals());

describe("the login key", () => {
  test("is kept from the login's answer and sent on every request after", async () => {
    await api.me().catch(() => {});
    expect(sent(0)).toBeNull();
    answers = [{ status: 200, body: { ok: true, login_key: KEY } }];
    await pageApi.login("alex", "admin password");
    expect(loginKey()).toBe(KEY);
    await api.me();
    await pageApi.approve("s1", "a1", "deny");
    await api.events("s1");
    expect([sent(2), sent(3), sent(4)]).toEqual([KEY, KEY, KEY]);
    // A JSON body keeps its type beside the key.
    expect(new Headers(fetchFn.mock.calls[3]![1]!.headers).get("content-type")).toBe("application/json");
  });

  test("a password change's new login replaces it", async () => {
    localStorage.setItem("harness.login", KEY);
    answers = [{ status: 200, body: { ok: true, login_key: "b".repeat(64) } }];
    await pageApi.changePassword("old", "a new password");
    expect(sent(0)).toBe(KEY);
    expect(loginKey()).toBe("b".repeat(64));
  });

  test("a 401 forgets it and signs out; a 403 does neither", async () => {
    localStorage.setItem("harness.login", KEY);
    const ended = vi.fn();
    login.ended = ended;
    answers = [{ status: 403, body: { error: "current password is wrong" } }];
    await expect(call("POST", "/api/me/password", {})).rejects.toThrow("current password is wrong");
    expect([loginKey(), ended.mock.calls.length]).toEqual([KEY, 0]);
    answers = [{ status: 401, body: { error: "not logged in" } }];
    await expect(api.sessions()).rejects.toThrow("not logged in");
    expect([loginKey(), ended.mock.calls.length]).toEqual([null, 1]);
  });

  test("a new tab of this origin has it, until any tab signs out", async () => {
    // Another tab logged in: same origin, same storage.
    localStorage.setItem("harness.login", KEY);
    await api.sessions();
    expect(sent(0)).toBe(KEY);
    forgetLogin();
    await api.sessions();
    expect(sent(1)).toBeNull();
  });

  test("requests outside call carry it too", async () => {
    expect(keyed({ a: "b" })).toEqual({ a: "b" });
    localStorage.setItem("harness.login", KEY);
    expect(keyed({ a: "b" })).toEqual({ a: "b", "x-harness-login": KEY });
    answers = [{ status: 200, body: { text: "hi" } }];
    await api.transcribe(new Blob(["x"], { type: "audio/webm" }));
    expect(sent(0)).toBe(KEY);
  });

  test("never goes into a URL", async () => {
    localStorage.setItem("harness.login", KEY);
    await api.events("s1", 3);
    await api.search("q");
    for (const [path] of fetchFn.mock.calls) expect(String(path)).not.toContain(KEY);
  });
});

describe("settings", () => {
  // The server only takes the presets' names, so a saved value that is not
  // one of them must not come back to be saved again.
  test("an accent that is not a preset comes back as the default", async () => {
    answers = [
      { status: 200, body: { search_url: "https://e.com/?q={q}", accent: "violet" } },
      { status: 200, body: { search_url: "https://e.com/?q={q}", accent: "gray" } },
      { status: 200, body: { search_url: "https://e.com/?q={q}", accent: "orange" } },
    ];
    expect((await api.settings()).accent).toBeNull();
    expect((await api.settings()).accent).toBeNull();
    expect((await api.settings()).accent).toBe("orange");
  });
});

describe("a request that gets no answer", () => {
  const calm = "The server is not reachable. Check that it is running, then try again.";

  test("reads as one calm sentence, not the browser's TypeError text", async () => {
    fetchFn.mockRejectedValueOnce(new TypeError("Failed to fetch"));
    await expect(api.me()).rejects.toThrow(calm);
    fetchFn.mockRejectedValueOnce(new TypeError("NetworkError when attempting to fetch resource."));
    await expect(call("GET", "/api/x")).rejects.toThrow(calm);
  });

  test("never carries the URL or the body", async () => {
    fetchFn.mockRejectedValueOnce(new TypeError("Failed to fetch https://h/api/x?token=SECRET"));
    const e = await call("POST", "/api/x", { password: "hunter2" }).catch((x: Error) => x);
    expect(String((e as Error).message)).not.toMatch(/SECRET|hunter2|https/);
  });

  test("a real HTTP error keeps its own message", async () => {
    answers = [{ status: 500, body: { error: "disk full", code: "disk" } }];
    const e = (await api.me().catch((x: unknown) => x)) as { message: string; status: number; code: string };
    expect([e.message, e.status, e.code]).toEqual(["disk full", 500, "disk"]);
    answers = [{ status: 502, body: null }];
    await expect(api.me()).rejects.toThrow(/^502/);
  });

  test("a caller's own abort is passed on unchanged", async () => {
    const abort = new DOMException("aborted", "AbortError");
    fetchFn.mockRejectedValueOnce(abort);
    await expect(call("GET", "/api/x")).rejects.toBe(abort);
  });

  test("keeps the original error reachable as `cause`", async () => {
    const original = new TypeError("Failed to execute 'fetch': Request with GET/HEAD method cannot have body.");
    fetchFn.mockRejectedValueOnce(original);
    const e = (await call("GET", "/api/x").catch((x: unknown) => x)) as Error;
    expect(e.message).toBe(calm);
    expect(e.cause).toBe(original);
  });

  // The three uploads that do not go through call() say the same thing.
  const uploads: [string, () => Promise<unknown>][] = [
    ["transcribe", () => api.transcribe(new Blob(["a"], { type: "audio/webm" }))],
    ["attachment upload", () => api.upload(new File(["a"], "a.txt"))],
    ["avatar upload", () => pageApi.setProfileImage("avatar", new Blob(["a"], { type: "image/png" }))],
  ];
  test.each(uploads)("%s: no answer reads as the calm sentence, with the cause kept", async (_n, run) => {
    const original = new TypeError("Failed to fetch");
    fetchFn.mockRejectedValueOnce(original);
    const e = (await run().catch((x: unknown) => x)) as Error;
    expect(e.message).toBe(calm);
    expect(e.cause).toBe(original);
  });
  test.each(uploads)("%s: a real HTTP error keeps its own message", async (_n, run) => {
    answers = [{ status: 413, body: { error: "too big" } }];
    await expect(run()).rejects.toThrow("too big");
  });
});

describe("the request id on an error", () => {
  const withHeaders = (status: number, body: unknown, headers: Record<string, string>) =>
    fetchFn.mockResolvedValueOnce(new Response(JSON.stringify(body), { status, headers }));
  test("is read from the body", async () => {
    answers = [{ status: 409, body: { error: "taken", request_id: "3f2a9c0e11223344" } }];
    const e = (await api.me().catch((x: unknown) => x)) as { id?: string };
    expect(e.id).toBe("3f2a9c0e11223344");
  });
  test("is read from the header when the body has none (a plain-text refusal too)", async () => {
    withHeaders(403, { error: "no" }, { "x-request-id": "aaaabbbbccccdddd" });
    expect(((await api.me().catch((x: unknown) => x)) as { id?: string }).id).toBe("aaaabbbbccccdddd");
    fetchFn.mockResolvedValueOnce(new Response("origin required", { status: 403, headers: { "x-request-id": "1111222233334444" } }));
    expect(((await api.me().catch((x: unknown) => x)) as { id?: string }).id).toBe("1111222233334444");
  });
  test("an error without one has none, and a 429 carries its Retry-After", async () => {
    answers = [{ status: 409, body: { error: "taken" } }];
    expect(((await api.me().catch((x: unknown) => x)) as { id?: string }).id).toBeUndefined();
    withHeaders(429, { error: "slow down" }, { "retry-after": "5" });
    expect(((await api.me().catch((x: unknown) => x)) as { retry?: number }).retry).toBe(5);
  });
});

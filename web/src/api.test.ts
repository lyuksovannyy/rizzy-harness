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

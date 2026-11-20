// @vitest-environment happy-dom
// Live topics: any number of listeners per topic, each told on its
// `<topic>_changed` and again after a reconnect, never for another topic.
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { Live } from "./live";

class FakeSocket {
  static all: FakeSocket[] = [];
  static OPEN = 1;
  readyState = 1;
  onopen: (() => void) | null = null;
  onmessage: ((m: { data: string }) => void) | null = null;
  onclose: (() => void) | null = null;
  constructor(public url: string) {
    FakeSocket.all.push(this);
  }
  sent: string[] = [];
  send(m: string) {
    this.sent.push(m);
  }
  close() {
    this.onclose?.();
  }
  say(msg: object) {
    this.onmessage?.({ data: JSON.stringify(msg) });
  }
}

beforeEach(() => {
  FakeSocket.all = [];
  vi.useFakeTimers();
  vi.stubGlobal("WebSocket", FakeSocket);
});
afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

describe("Live.onTopic", () => {
  test("tells every listener of a topic, until it unsubscribes", () => {
    const live = new Live();
    const ws = FakeSocket.all[0]!;
    ws.onopen!();
    const shell = vi.fn();
    const page = vi.fn();
    const media = vi.fn();
    live.onTopic("routines", shell);
    const off = live.onTopic("routines", page);
    live.onTopic("media", media);
    ws.say({ type: "routines_changed" });
    expect([shell.mock.calls.length, page.mock.calls.length, media.mock.calls.length]).toEqual([1, 1, 0]);
    off();
    ws.say({ type: "routines_changed" });
    ws.say({ type: "media_changed" });
    expect([shell.mock.calls.length, page.mock.calls.length, media.mock.calls.length]).toEqual([2, 1, 1]);
    live.close();
  });

  test("tells them again after a reconnect, not on the first connect", () => {
    const live = new Live();
    const artifacts = vi.fn();
    live.onTopic("artifacts", artifacts);
    FakeSocket.all[0]!.onopen!();
    expect(artifacts).not.toHaveBeenCalled();
    FakeSocket.all[0]!.close();
    vi.advanceTimersByTime(1000);
    FakeSocket.all[1]!.onopen!();
    expect(artifacts).toHaveBeenCalledTimes(1);
    live.close();
  });
});

describe("Live.onLoginEnded", () => {
  // 4401: the server ended this socket's login (logout elsewhere, a password
  // change or reset, a disabled account). The shell checks /api/me and signs
  // out (clearing drafts) when the login is really gone.
  test("is told when the server closes the socket for an ended login, not otherwise", () => {
    const live = new Live();
    const ended = vi.fn();
    live.onLoginEnded = ended;
    const close = (code: number) => (FakeSocket.all.at(-1)!.onclose as unknown as (e: { code: number }) => void)({ code });
    close(1006);
    expect(ended).not.toHaveBeenCalled();
    vi.runOnlyPendingTimers();
    close(4401);
    expect(ended).toHaveBeenCalledTimes(1);
    live.close();
  });

  test("after 4401 the socket does not knock again by itself; any other close does", () => {
    const live = new Live();
    const close = (code: number) => (FakeSocket.all.at(-1)!.onclose as unknown as (e: { code: number }) => void)({ code });
    close(4401);
    vi.advanceTimersByTime(60_000);
    expect(FakeSocket.all).toHaveLength(1);
    // An explicit connect (the shell, once the login is known good) works.
    live.connect();
    expect(FakeSocket.all).toHaveLength(2);
    close(1006);
    vi.advanceTimersByTime(60_000);
    expect(FakeSocket.all).toHaveLength(3);
    live.close();
  });
});

describe("Live.retryAfterLogin", () => {
  // A cookie that is good while the socket keeps being closed with 4401 (a
  // lost login key, for one) must not turn into a loop of /api/me and new
  // sockets: each retry waits, twice as long as the one before.
  test("waits before each new socket, longer each time, and starts over once a socket delivers", () => {
    const live = new Live();
    const last = () => FakeSocket.all.at(-1)!;
    const close4401 = () => (last().onclose as unknown as (e: { code: number }) => void)({ code: 4401 });
    const waits: number[] = [];
    for (let i = 0; i < 4; i++) {
      close4401();
      const before = FakeSocket.all.length;
      live.retryAfterLogin();
      expect(FakeSocket.all).toHaveLength(before); // never at once
      let waited = 0;
      while (FakeSocket.all.length === before && waited < 120_000) {
        vi.advanceTimersByTime(100);
        waited += 100;
      }
      waits.push(waited);
    }
    expect(waits).toEqual([500, 1000, 2000, 4000]);
    // A socket that delivers a message proves the login: the wait starts over.
    last().onopen!();
    last().onmessage!({ data: JSON.stringify({ type: "sessions_changed" }) } as MessageEvent);
    close4401();
    const before = FakeSocket.all.length;
    live.retryAfterLogin();
    vi.advanceTimersByTime(499);
    expect(FakeSocket.all).toHaveLength(before);
    vi.advanceTimersByTime(1);
    expect(FakeSocket.all).toHaveLength(before + 1);
    // A second call while one is waiting or open adds no socket.
    live.retryAfterLogin();
    vi.advanceTimersByTime(120_000);
    expect(FakeSocket.all).toHaveLength(before + 1);
    live.close();
  });
});

describe("the login key (Gate 3 P-1)", () => {
  test("is the first thing every socket sends, before any subscribe, and never in its URL", () => {
    const key = "c".repeat(64);
    localStorage.setItem("harness.login", key);
    const live = new Live();
    const ws = FakeSocket.all[0]!;
    ws.readyState = 0; // still connecting, as a real socket is here
    live.watch("s1");
    ws.readyState = 1;
    ws.onopen!();
    expect(ws.sent.map((m) => JSON.parse(m).type)).toEqual(["login", "subscribe"]);
    expect(JSON.parse(ws.sent[0]!)).toEqual({ type: "login", key });
    expect(ws.url).not.toContain(key);
    // A reconnect sends it again, first.
    ws.close();
    vi.advanceTimersByTime(600);
    const again = FakeSocket.all[1]!;
    again.onopen!();
    expect(JSON.parse(again.sent[0]!)).toEqual({ type: "login", key });
    live.close();
    localStorage.clear();
  });
});

describe("Live.watch", () => {
  test("is counted: the subscription ends with the last unwatch", () => {
    const live = new Live();
    const ws = FakeSocket.all[0]!;
    ws.onopen!();
    const subs = () => ws.sent.map((m) => JSON.parse(m) as { type: string }).filter((m) => m.type !== "login");
    live.watch("child");
    live.watch("child");
    expect(subs().map((m) => m.type)).toEqual(["subscribe"]);
    live.unwatch("child");
    expect(live.view("child")).toBeDefined();
    expect(subs().map((m) => m.type)).toEqual(["subscribe"]);
    live.unwatch("child");
    expect(live.view("child")).toBeUndefined();
    expect(subs().map((m) => m.type)).toEqual(["subscribe", "unsubscribe"]);
    live.close();
  });
});

describe("the fold loads with the chat, not with the first screen", () => {
  const SID = "s1";
  const event = (seq: number, kind: string, payload: object) => ({
    type: "event",
    event: { session_id: SID, seq, ts_ms: 0, kind, payload },
  });

  test("events that come before it has loaded are folded in order once it has, duplicates once", async () => {
    const live = new Live();
    const ws = FakeSocket.all[0]!;
    ws.onopen!();
    live.watch(SID);
    expect(live.view(SID)!.head_seq).toBe(0);
    // Order matters here: held in reverse, 3 and 2 would be gaps.
    ws.say(event(1, "turn_started", { turn_id: "t" }));
    ws.say(event(2, "user_message", { turn_id: "t", text: "hello" }));
    ws.say(event(1, "turn_started", { turn_id: "t" }));
    ws.say(event(3, "user_message", { turn_id: "t", text: "again" }));
    await vi.dynamicImportSettled();
    const v = live.view(SID)!;
    expect(v.head_seq).toBe(3);
    expect(v.items).toEqual([
      { type: "user", turn_id: "t", text: "hello" },
      { type: "user", turn_id: "t", text: "again" },
    ]);
    // After it is here, events fold as they come.
    ws.say(event(4, "error", { message: "boom" }));
    expect(live.view(SID)!.items.at(-1)).toEqual({ type: "error", message: "boom" });
    live.close();
  });

  test("a load that fails is tried again at the next watch, which asks the server again", async () => {
    const live = new Live();
    const ws = FakeSocket.all[0]!;
    ws.onopen!();
    const real = live.loadModule;
    const load = vi.fn(() => Promise.reject(new Error("chunk gone")));
    live.loadModule = load;
    const logged = vi.spyOn(console, "error").mockImplementation(() => {});
    live.watch(SID);
    ws.say(event(1, "turn_started", { turn_id: "t" }));
    ws.say(event(2, "user_message", { turn_id: "t", text: "hello" }));
    await vi.waitFor(() => expect(logged).toHaveBeenCalledTimes(1));
    // Events that come meanwhile do not each try again (no request per delta).
    ws.say(event(3, "error", { message: "boom" }));
    expect(load).toHaveBeenCalledTimes(1);
    expect(live.view(SID)!.head_seq).toBe(0);
    live.loadModule = real;
    ws.sent.length = 0;
    live.watch("s2");
    await vi.dynamicImportSettled();
    await vi.waitFor(() => expect(ws.sent.map((m) => JSON.parse(m))).toContainEqual({ type: "subscribe", session_id: SID, after_seq: 0 }));
    logged.mockRestore();
    live.close();
  });
});

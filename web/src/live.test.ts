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

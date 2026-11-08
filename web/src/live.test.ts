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
  send() {}
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

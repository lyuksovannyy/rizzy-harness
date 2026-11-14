// @vitest-environment happy-dom
// "Finished, unread": one notification per chat that newly finished while
// the person looks elsewhere, with fixed words and the chat's place in the
// list (never its title); none for what was unread before the tab opened;
// none for the chat they are looking at, which is marked seen instead.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import type { SessionRow } from "../types";

const markSeen = vi.fn((_id: string, _seq: number) => Promise.resolve({ ok: true }));
vi.mock("../api", () => ({ api: { markSeen: (id: string, seq: number) => markSeen(id, seq) } }));
const { newlyFinished, useFinishedSignal } = await import("./finished");

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const chat = (id: string, extra: Partial<SessionRow> = {}): SessionRow => ({
  session_id: id,
  node_id: "master",
  title: `Secret plan ${id}`,
  cwd: "/home/alex/secret",
  model: "m",
  created_ms: 0,
  updated_ms: 0,
  head_seq: 7,
  running: false,
  owner_id: "u1",
  agent_id: null,
  profile: "chat",
  incognito: false,
  parent_id: null,
  ...extra,
});

const shown: { title: string; body?: string; tag?: string }[] = [];
class FakeNotification {
  static permission = "granted";
  onclick: (() => void) | null = null;
  constructor(title: string, opts: { body?: string; tag?: string }) {
    shown.push({ title, ...opts });
  }
  close() {}
}

function Probe(p: { rows: SessionRow[] | null; viewing: string | null }) {
  useFinishedSignal(p.rows, p.viewing);
  return null;
}

let host: HTMLElement;
let root: Root;
const show = (rows: SessionRow[] | null, viewing: string | null = null) => act(() => root.render(<Probe rows={rows} viewing={viewing} />));

beforeEach(() => {
  shown.length = 0;
  markSeen.mockClear();
  vi.stubGlobal("Notification", FakeNotification);
  Object.defineProperty(window, "isSecureContext", { value: true, configurable: true });
  localStorage.setItem("harness.notify", "1");
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
  vi.unstubAllGlobals();
  localStorage.clear();
});

describe("finished, unread", () => {
  test("newly finished: unread, not running, not unread before", () => {
    const rows = [chat("a", { unread: true }), chat("b", { unread: true, running: true }), chat("c", { unread: true }), chat("d")];
    expect(newlyFinished(new Set(["c"]), rows).map((r) => r.session_id)).toEqual(["a"]);
  });

  test("one notification per newly finished chat, with fixed words and its place in the list", async () => {
    await show(null);
    await show([chat("a"), chat("b")]);
    expect(shown).toEqual([]);
    await show([chat("a"), chat("b", { unread: true })]);
    expect(shown).toHaveLength(1);
    expect(shown[0]!.body).toBe("A chat finished: number 2 in your recent chats");
    const all = JSON.stringify(shown[0]);
    expect(all).not.toContain("Secret");
    expect(all).not.toContain("/home");
    // Still unread on the next list: not told again.
    await show([chat("a"), chat("b", { unread: true })]);
    expect(shown).toHaveLength(1);
  });

  test("what was unread before the tab opened is never notified", async () => {
    await show(null);
    await show([chat("a", { unread: true })]);
    expect(shown).toEqual([]);
  });

  test("the chat being looked at is not notified and is marked seen", async () => {
    await show([chat("a")], "a");
    await show([chat("a", { unread: true, head_seq: 12 })], "a");
    expect(shown).toEqual([]);
    expect(markSeen).toHaveBeenCalledWith("a", 12);
  });

  test("nothing without the person's permission", async () => {
    localStorage.setItem("harness.notify", "0");
    await show([chat("a")]);
    await show([chat("a", { unread: true })]);
    expect(shown).toEqual([]);
  });
});

import { describe, expect, test, vi } from "vitest";
import type { SessionRow } from "../types";
import { appendPage, applyRefetch, coalesce, combine, type List, refetchLimit } from "./chatPages";

const row = (n: number, more: Partial<SessionRow> = {}): SessionRow => ({
  session_id: `00000000-0000-7000-8000-${String(n).padStart(12, "0")}`,
  node_id: "master",
  title: `chat ${n}`,
  cwd: "",
  model: "m",
  created_ms: n,
  updated_ms: n,
  head_seq: 1,
  running: false,
  owner_id: "u",
  agent_id: null,
  profile: "chat",
  incognito: false,
  parent_id: null,
  ...more,
});
/** Rows `from` down to `to`, newest first. */
const span = (from: number, to: number) => Array.from({ length: from - to + 1 }, (_, i) => row(from - i));
const names = (rows: SessionRow[]) => rows.map((r) => r.updated_ms);

describe("chat pages", () => {
  test("a refetch asks for what is loaded, at least a page, at most 200", () => {
    expect(refetchLimit(0, 50)).toBe(50);
    expect(refetchLimit(120, 50)).toBe(120);
    expect(refetchLimit(450, 50)).toBe(200);
  });

  test("scrolled pages append; a row listed again leaves its old place", () => {
    let l = appendPage(null, { sessions: span(100, 91), children: [], next: "c1" });
    l = appendPage(l, { sessions: [row(95, { updated_ms: 89 }), ...span(88, 80)], children: [], next: "c2" });
    // 20 rows fetched, one of them twice: 19, the moved one in its new place.
    expect(l.rows).toHaveLength(19);
    expect(names(l.rows).slice(0, 10)).toEqual([100, 99, 98, 97, 96, 94, 93, 92, 91, 89]);
    expect(l.next).toBe("c2");
  });

  test("a refetch replaces the rows it covers and keeps the ones past it", () => {
    // 250 loaded; the refetch covers the newest 200.
    const loaded: List = { rows: span(1_000, 751), children: [row(5_000, { parent_id: row(760).session_id })], next: "old" };
    const fresh = span(1_000, 801);
    const l = applyRefetch(loaded, { sessions: fresh, children: [], next: "new" });
    expect(l.rows).toHaveLength(250);
    expect(names(l.rows)).toEqual(names(span(1_000, 751)));
    expect(l.next).toBe("old");
    expect(l.children).toHaveLength(1);
  });

  test("a chat that moved to the top shows there once, from wherever it was", () => {
    const loaded: List = { rows: span(300, 51), children: [], next: "old" };
    const moved = row(60, { updated_ms: 400 });
    const fresh = [moved, ...span(300, 102)];
    const l = applyRefetch(loaded, { sessions: fresh, children: [], next: "new" });
    expect(l.rows[0]).toBe(moved);
    expect(l.rows.filter((r) => r.session_id === moved.session_id)).toHaveLength(1);
    // 101..51 stay, but the moved one only on top.
    expect(l.rows).toHaveLength(250);
  });

  test("a deleted chat inside the refetch disappears; a short list ends", () => {
    const loaded: List = { rows: span(10, 1), children: [], next: null };
    const without5 = span(10, 1).filter((r) => r.updated_ms !== 5);
    const l = applyRefetch(loaded, { sessions: without5, children: [], next: null });
    expect(names(l.rows)).not.toContain(5);
    expect(l.next).toBeNull();
    // Within one page: no tail, the new cursor.
    const one = applyRefetch({ rows: span(60, 11), children: [], next: "a" }, { sessions: span(61, 12), children: [], next: "b" });
    expect(one.next).toBe("a");
    expect(names(one.rows)).toEqual(names(span(61, 11)));
    const covered = applyRefetch({ rows: span(60, 11), children: [], next: "a" }, { sessions: span(70, 21), children: [], next: "b" });
    expect(covered.next).toBe("a");
  });

  test("sub-agent chats follow their top-level chat: refreshed with it, kept past the refetch", () => {
    const top = row(100);
    const tailTop = row(10);
    const kid = row(101, { parent_id: top.session_id });
    const tailKid = row(11, { parent_id: tailTop.session_id });
    const grandKid = row(12, { parent_id: tailKid.session_id });
    const loaded: List = { rows: [top, ...span(99, 50), tailTop], children: [kid, tailKid, grandKid], next: "old" };
    const kid2 = { ...kid, running: true, head_seq: 2 };
    const l = applyRefetch(loaded, { sessions: [top, ...span(99, 50)], children: [kid2], next: "new" });
    expect(l.children.map((r) => r.session_id).sort()).toEqual([kid.session_id, tailKid.session_id, grandKid.session_id].sort());
    expect(l.children.find((r) => r.session_id === kid.session_id)?.running).toBe(true);
  });

  test("the lists combine once each, the freshest copy, newest first, with the open chat", () => {
    const a = row(5);
    const newer = { ...a, head_seq: 4, running: true };
    const open = row(1);
    const all = combine(
      [
        { rows: [row(9), a], children: [], next: null },
        { rows: [newer, row(7)], children: [row(8, { parent_id: row(9).session_id })], next: null },
      ],
      open,
    );
    expect(names(all)).toEqual([9, 8, 7, 5, 1]);
    expect(all.find((r) => r.session_id === a.session_id)?.running).toBe(true);
    expect(combine([{ rows: [open], children: [], next: null }], open)).toHaveLength(1);
  });

  test("a burst of nudges is one refetch in flight and one after it", async () => {
    const done: (() => void)[] = [];
    const run = vi.fn(() => new Promise<void>((r) => done.push(r)));
    const go = coalesce(run);
    go();
    go();
    go();
    go();
    expect(run).toHaveBeenCalledTimes(1);
    done[0]!();
    await vi.waitFor(() => expect(run).toHaveBeenCalledTimes(2));
    go();
    done[1]!();
    await vi.waitFor(() => expect(run).toHaveBeenCalledTimes(3));
    done[2]!();
    await Promise.resolve();
    expect(run).toHaveBeenCalledTimes(3);
    // A failed run does not stop the next.
    const failing = vi.fn(() => Promise.reject(new Error("x")));
    const f = coalesce(failing);
    f();
    await vi.waitFor(() => expect(failing).toHaveBeenCalledTimes(1));
    f();
    await vi.waitFor(() => expect(failing).toHaveBeenCalledTimes(2));
  });
});

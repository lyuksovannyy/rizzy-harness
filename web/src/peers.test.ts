// X.37 in the browser's fold: the turn of an agent a message called after
// the first, the note for agents that were not asked, and what a hostile or
// older node may send in their place (nothing crashes, nothing is half shown).
import { describe, expect, it } from "vitest";
import { apply, fold } from "./fold";
import type { RawEvent } from "./types";

const ev = (seq: number, kind: string, payload: object): RawEvent => ({ session_id: "s", seq, ts_ms: 0, kind, payload }) as RawEvent;
const agent = (id: string) => ({ agent_id: id, name: id, prompt_hash: "h" });

const list = [
  ev(1, "turn_started", { turn_id: "t1", agent: agent("coder") }),
  ev(2, "user_message", { turn_id: "t1", text: "@coder @writer hi", triggers: ["coder", "writer", "editor"] }),
  ev(3, "turn_ended", { turn_id: "t1", reason: "completed" }),
  ev(4, "turn_started", { turn_id: "t2", agent: agent("writer"), cause: { mention: { message_seq: 2, by: "person" } } }),
  ev(5, "turn_ended", { turn_id: "t2", reason: "cancelled" }),
  ev(6, "peers_skipped", { message_seq: 2, agents: ["editor"], reason: "stopped" }),
];

describe("a message that called several agents", () => {
  it("is a user item with who it called, a called item where the second answers, and the note", () => {
    const v = fold(list);
    expect(v.unknown_events).toBe(0);
    expect(v.items.map((i) => i.type)).toEqual(["user", "turn_end", "called", "turn_end", "peers_skipped"]);
    expect(v.items[0]).toMatchObject({ triggers: ["coder", "writer", "editor"] });
    expect(v.items[2]).toMatchObject({ type: "called", turn_id: "t2", message_seq: 2, agent: { agent_id: "writer" } });
    expect(v.items[4]).toMatchObject({ agents: ["editor"], reason: "stopped" });
    // It is no callback: nobody is shown a "background command ended" line.
    expect(v.items.some((i) => i.type === "callback")).toBe(false);
  });

  it("a mention turn for an agent the master never sent is drawn, never a crash", () => {
    const v = fold([
      ev(1, "turn_started", { turn_id: "t1" }),
      ev(2, "turn_ended", { turn_id: "t1", reason: "completed" }),
      ev(3, "turn_started", { turn_id: "t2", agent: agent("not-in-the-list"), cause: { mention: { message_seq: 99, by: "person" } } }),
      ev(4, "turn_started", { turn_id: "t3", cause: { mention: { message_seq: 1, by: "person" } } }),
    ]);
    expect(v.unknown_events).toBe(0);
    expect(v.items.filter((i) => i.type === "called")).toHaveLength(2);
    expect(v.items[2]).toMatchObject({ agent: null });
  });

  it("drops a cause that is no one variant, as serde does", () => {
    const base = [ev(1, "turn_started", { turn_id: "t1" }), ev(2, "turn_ended", { turn_id: "t1", reason: "completed" })];
    for (const cause of [{}, { callback: ["bg-1"], mention: { message_seq: 1, by: "person" } }, { future: 1 }, { mention: { message_seq: 1, by: "robot" } }, { mention: { by: "person" } }]) {
      const v = fold([...base, ev(3, "turn_started", { turn_id: "t2", cause })]);
      expect(v.unknown_events, JSON.stringify(cause)).toBe(1);
      expect(v.items.map((i) => i.type)).toEqual(["turn_end"]);
      expect(v.running_turn).toBeNull();
    }
  });

  it("drops a note with an unknown reason and one without agents", () => {
    for (const p of [{ message_seq: 1, agents: ["a"], reason: "bored" }, { message_seq: 1, reason: "stopped" }]) {
      const v = fold([ev(1, "peers_skipped", p)]);
      expect(v.unknown_events).toBe(1);
      expect(v.items).toEqual([]);
    }
  });

  it("applies one event at a time as the live view does", () => {
    const v = fold(list.slice(0, 3));
    for (const e of list.slice(3)) apply(v, e);
    expect(v.head_seq).toBe(6);
    expect(v.items.at(-1)?.type).toBe("peers_skipped");
  });
});

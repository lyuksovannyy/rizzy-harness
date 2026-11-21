import { describe, expect, it } from "vitest";
import { apply, fold } from "./fold";
import type { RawEvent } from "./types";

// X.45: the fold marks the items a rewind took out of the conversation with
// the rewind's own seq (the Rust fold does the same, pinned by the shared
// fixture); state folds as it did.
const ev = (seq: number, kind: string, payload: object): RawEvent => ({ session_id: "s", seq, ts_ms: 0, kind, payload }) as RawEvent;
const turn = (at: number, id: string, text: string): RawEvent[] => [
  ev(at, "turn_started", { turn_id: id }),
  ev(at + 1, "user_message", { turn_id: id, text }),
  ev(at + 2, "assistant_message", { turn_id: id, message_id: `m-${id}`, text: `${id} answer` }),
  ev(at + 3, "turn_ended", { turn_id: id, reason: "completed" }),
];

describe("rewind", () => {
  it("marks the rewound range with the rewind's seq and an undo clears it", () => {
    const log = [...turn(1, "t1", "one"), ...turn(5, "t2", "two")];
    const v = fold([...log, ev(9, "conversation_rewound", { message_seq: 6, turn_id: "t2", files: false })]);
    const marks = v.items.map((i) => i.rewound);
    expect(marks).toEqual([undefined, undefined, undefined, 9, 9, 9]);
    expect(v.items[0]).not.toHaveProperty("rewound");
    expect(v.rewinds).toEqual([{ seq: 9, message_seq: 6, turn_id: "t2", turns: 1, files: false, undone: false, can_undo: true }]);
    apply(v, ev(10, "conversation_rewind_undone", { rewind_seq: 9 }));
    expect(v.items.map((i) => i.rewound)).toEqual([undefined, undefined, undefined, undefined, undefined, undefined]);
    expect(v.items.every((i) => !("rewound" in i))).toBe(true);
    expect(v.rewinds![0]).toMatchObject({ undone: true, can_undo: false });
  });

  it("an item is shown under the earliest rewind that hides it, and a turn after one ends its undo", () => {
    const log = [...turn(1, "t1", "one"), ...turn(5, "t2", "two")];
    const v = fold([
      ...log,
      ev(9, "conversation_rewound", { message_seq: 6, turn_id: "t2" }),
      ...turn(10, "t3", "three"),
      ev(14, "conversation_rewound", { message_seq: 2, turn_id: "t1", files: true }),
    ]);
    // t2's items stay under the first rewind; the rest of the range is the second's.
    expect(v.items.map((i) => i.rewound)).toEqual([14, 14, 14, 9, 9, 9, 14, 14, 14]);
    expect(v.rewinds!.map((r) => [r.seq, r.can_undo, r.turns])).toEqual([
      [9, false, 1],
      [14, true, 3],
    ]);
    apply(v, ev(15, "conversation_rewind_undone", { rewind_seq: 14 }));
    expect(v.items.map((i) => i.rewound)).toEqual([undefined, undefined, undefined, 9, 9, 9, undefined, undefined, undefined]);
  });

  it("a late event for a rewound turn folds and does not show the turn again", () => {
    const log = [...turn(1, "t1", "one"), ev(5, "turn_started", { turn_id: "t2" }), ev(6, "user_message", { turn_id: "t2", text: "serve" })];
    const v = fold([
      ...log,
      ev(7, "background_started", { bg_id: "bg-1", call_id: "c", turn_id: "t2", timeout_s: 60, ends_by_ms: 0 }),
      ev(8, "turn_ended", { turn_id: "t2", reason: "completed" }),
      ev(9, "conversation_rewound", { message_seq: 6, turn_id: "t2" }),
      ev(10, "background_ended", { bg_id: "bg-1", reason: "exited", confirmed: "ended", ran_ms: 1, output_bytes: 0 }),
    ]);
    expect(v.unknown_events).toBe(0);
    expect(v.items.filter((i) => i.type === "user").map((i) => i.rewound)).toEqual([undefined, 9]);
    // State folds as it did: the command ended.
    expect(v.background?.["bg-1"]?.ended).toBe(true);
    expect(v.head_seq).toBe(10);
  });

  it("a malformed rewind is counted, not shown, and breaks nothing", () => {
    const v = fold([...turn(1, "t1", "one"), ev(5, "conversation_rewound", { message_seq: "x", turn_id: "t1" })]);
    expect(v.unknown_events).toBe(1);
    expect(v.rewinds).toBeUndefined();
    expect(v.items.every((i) => i.rewound === undefined)).toBe(true);
  });
});

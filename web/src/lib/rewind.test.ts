import { describe, expect, it } from "vitest";
import { fold } from "../fold";
import type { Item, RawEvent, SessionView } from "../types";
import { changesFiles, forceable, noRewind, resend, runs, type RewindWhere } from "./rewind";

// X.46: when "Rewind to here" shows, what a message gives back to the box,
// and how the rewound items group into runs.
const ev = (seq: number, kind: string, payload: object): RawEvent => ({ session_id: "s", seq, ts_ms: 0, kind, payload }) as RawEvent;
const turn = (at: number, id: string, text: string, extra: object = {}): RawEvent[] => [
  ev(at, "turn_started", { turn_id: id }),
  ev(at + 1, "user_message", { turn_id: id, text, ...extra }),
  ev(at + 2, "assistant_message", { turn_id: id, message_id: `m-${id}`, text: `${id} answer` }),
  ev(at + 3, "turn_ended", { turn_id: id, reason: "completed" }),
];
const ok: RewindWhere = { mine: true, subagent: false, routine: false, compacting: false };
const userAt = (v: SessionView, n: number) => v.items.map((it, i) => [it, i] as const).filter(([it]) => it.type === "user")[n]![1];

describe("noRewind: one answer per reason the action is not offered", () => {
  const log = [...turn(1, "t1", "one"), ...turn(5, "t2", "two")];
  it("offers the person's own message of an idle chat", () => {
    const v = fold(log);
    expect(noRewind(v, userAt(v, 0), ok)).toBeNull();
    expect(noRewind(v, userAt(v, 1), ok)).toBeNull();
  });
  it("not an item that is not a message", () => {
    const v = fold(log);
    expect(noRewind(v, 99, ok)).toBe("not-a-turn");
    expect(noRewind(v, v.items.findIndex((x) => x.type === "assistant"), ok)).toBe("not-a-turn");
  });
  it("not where the viewer does not drive the chat, nor a sub-agent's chat", () => {
    const v = fold(log);
    expect(noRewind(v, userAt(v, 1), { ...ok, mine: false })).toBe("viewer");
    expect(noRewind(v, userAt(v, 1), { ...ok, subagent: true })).toBe("sub-agent");
  });
  it("not a routine's own first message, but a later one", () => {
    const v = fold(log);
    expect(noRewind(v, userAt(v, 0), { ...ok, routine: true })).toBe("routine");
    expect(noRewind(v, userAt(v, 1), { ...ok, routine: true })).toBeNull();
  });
  it("not a message sent while an answer ran", () => {
    const v = fold([...log, ev(9, "turn_started", { turn_id: "t3" }), ev(10, "user_message", { turn_id: "t3", text: "x" }), ev(11, "user_message", { turn_id: "t3", text: "later", queued: true }), ev(12, "turn_ended", { turn_id: "t3", reason: "completed" })]);
    expect(noRewind(v, userAt(v, 2), ok)).toBeNull();
    expect(noRewind(v, userAt(v, 3), ok)).toBe("not-a-turn");
  });
  it("not one the fold did not make (no seq to name)", () => {
    const v = fold(log);
    const bare = { ...v, items: [{ type: "user", turn_id: "t", text: "x" } as Item] };
    expect(noRewind(bare, 0, ok)).toBe("unknown");
  });
  it("not one already rewound", () => {
    const v = fold([...log, ev(9, "conversation_rewound", { message_seq: 6, turn_id: "t2", files: false })]);
    expect(noRewind(v, userAt(v, 1), ok)).toBe("rewound");
    expect(noRewind(v, userAt(v, 0), ok)).toBeNull();
  });
  it("not one the latest summary holds; a summary a rewind took out does not count", () => {
    const sum = ev(9, "history_summarized", { turn_id: "t2", through_seq: 4, text: "s" });
    const v = fold([...log, sum]);
    expect(noRewind(v, userAt(v, 0), ok)).toBe("compacted");
    expect(noRewind(v, userAt(v, 1), ok)).toBeNull();
    const w = fold([...log, sum, ev(10, "conversation_rewound", { message_seq: 6, turn_id: "t2", files: false })]);
    expect(noRewind(w, userAt(w, 0), ok)).toBeNull();
  });
  it("not while a turn, a /compact, a background command or a background sub-agent runs", () => {
    const run = fold([...log, ev(9, "turn_started", { turn_id: "t3" })]);
    expect(noRewind(run, userAt(run, 0), ok)).toBe("busy");
    const idle = fold(log);
    expect(noRewind(idle, userAt(idle, 0), { ...ok, compacting: true })).toBe("busy");
    const bg = fold([...log, ev(9, "background_started", { bg_id: "bg-1", call_id: "c", turn_id: "t2", timeout_s: 5, ends_by_ms: 0 })]);
    expect(noRewind(bg, userAt(bg, 0), ok)).toBe("busy");
    const kid = fold([...log, ev(9, "background_started", { bg_id: "bg-1", call_id: "c", turn_id: "t2", timeout_s: 5, ends_by_ms: 0, child: { session: "x", agent_id: "a" } })]);
    expect(noRewind(kid, userAt(kid, 0), ok)).toBe("busy");
    const ended = fold([...log, ev(9, "background_started", { bg_id: "bg-1", call_id: "c", turn_id: "t2", timeout_s: 5, ends_by_ms: 0 }), ev(10, "background_ended", { bg_id: "bg-1", reason: "exited", ran_ms: 1, output_bytes: 0, confirmed: "ended" })]);
    expect(noRewind(ended, userAt(ended, 0), ok)).toBeNull();
  });
});

describe("changesFiles", () => {
  it("only when a turn from the message on changed files nobody put back", () => {
    const base = [...turn(1, "t1", "one"), ev(5, "checkpoint", { turn_id: "t1", phase: "after", git_ref: "r", files: ["a.rs"] }), ...turn(6, "t2", "two")];
    const v = fold(base);
    expect(changesFiles(v, userAt(v, 0))).toBe(true);
    expect(changesFiles(v, userAt(v, 1))).toBe(false);
    const none = fold([...turn(1, "t1", "one"), ev(5, "checkpoint", { turn_id: "t1", phase: "after", git_ref: "r", files: [] })]);
    expect(changesFiles(none, 0)).toBe(false);
  });
});

describe("resend: what a logged message can honestly be sent again with", () => {
  it("the text and the images it carried (Media references); the attachments are named, not resent", () => {
    const v = fold(turn(1, "t1", "look", { attachments: [{ name: "notes.txt", mime: "text/plain", text: "abcd" }], images: [{ media_id: "m1", sha256: "x", mime: "image/png", name: "shot.png", width: 1, height: 1 }] }));
    const r = resend(v.items.find((i): i is Extract<Item, { type: "user" }> => i.type === "user")!)!;
    expect(r.text).toBe("look");
    expect(r.lost).toEqual(["notes.txt"]);
    expect(r.pictures).toEqual([{ key: "m1", name: "shot.png", size: 0, media_id: "m1" }]);
  });
  it("a form's answer is not a message to edit", () => {
    expect(resend({ type: "user", turn_id: "t", text: "x", reply: {} as never })).toBeNull();
  });
});

describe("forceable", () => {
  it("knows the restore's refusals that a forced call goes past", () => {
    expect(forceable("these files changed after the turn; undo would overwrite that: `a`")).toBe(true);
    expect(forceable('in no checkpoint: `.env`; "Undo anyway" deletes them')).toBe(true);
    expect(forceable("This chat can't be rewound there: a turn is running")).toBe(false);
  });
});

describe("runs", () => {
  it("collapses consecutive rewound items under one rewind, counting turns", () => {
    const v = fold([...turn(1, "t1", "one"), ...turn(5, "t2", "two"), ...turn(9, "t3", "three"), ev(13, "conversation_rewound", { message_seq: 6, turn_id: "t2", files: false })]);
    expect(runs(v.items)).toEqual([{ rewind: 13, start: 3, end: 8, turns: 2 }]);
  });
  it("splits at an item another rewind holds", () => {
    const log = [...turn(1, "t1", "one"), ...turn(5, "t2", "two"), ev(9, "conversation_rewound", { message_seq: 6, turn_id: "t2", files: false }), ...turn(10, "t3", "three"), ev(14, "conversation_rewound", { message_seq: 2, turn_id: "t1", files: false })];
    const v = fold(log);
    expect(runs(v.items).map((r) => [r.rewind, r.turns])).toEqual([[14, 1], [9, 1], [14, 1]]);
  });
  it("none without a rewind", () => {
    expect(runs(fold(turn(1, "t1", "one")).items)).toEqual([]);
  });
});

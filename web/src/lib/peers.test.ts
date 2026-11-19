import { describe, expect, test } from "vitest";
import type { Item } from "../types";
import { peersLeft } from "./peers";

const user = (triggers?: string[], queued = false): Item => ({ type: "user", turn_id: "t", text: "x", ...(triggers ? { triggers } : {}), ...(queued ? { queued: { seq: 9, read: false } } : {}) });
const called = (id: string): Item => ({ type: "called", turn_id: id, agent: null, message_seq: 2 });

describe("peersLeft", () => {
  test("counts the agents of the last message that have not had their turn", () => {
    const l = ["a", "b", "c", "d"];
    expect(peersLeft([user(l)], true)).toBe(3);
    expect(peersLeft([user(l), called("t2")], true)).toBe(2);
    expect(peersLeft([user(l), called("t2"), called("t3"), called("t4")], true)).toBe(0);
  });
  test("not when idle, for a message that called one agent, after Stop's note, or past a queued message", () => {
    expect(peersLeft([user(["a", "b"])], false)).toBe(0);
    expect(peersLeft([user(["a"])], true)).toBe(0);
    expect(peersLeft([user()], true)).toBe(0);
    expect(peersLeft([user(["a", "b", "c"]), { type: "peers_skipped", agents: ["c"], reason: "stopped" }], true)).toBe(0);
    expect(peersLeft([user(["a", "b", "c"]), user(undefined, true), called("t2")], true)).toBe(1);
    expect(peersLeft([], true)).toBe(0);
  });
});

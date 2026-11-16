// X.24 review F7 (the reviewer's proof r_bg.test.ts): malformed `child`
// payloads from a node (the master's mirror does not type-check them) do
// not throw in the fold, the side panel's rows or the running list.
import { describe, expect, it } from "vitest";
import { fold } from "./fold";
import { sideItems } from "./components/sideItems";
import { runningBackground } from "./lib/background";
import type { RawEvent } from "./types";

const ev = (seq: number, kind: string, payload: unknown): RawEvent => ({ session_id: "s", seq, ts_ms: 0, kind, payload }) as RawEvent;

const bads: unknown[] = [null, 5, "x", [], { session: 7 }, { usage: 5 }, { usage: "x" }, { usage: { by_model: "abc" } }, { usage: { by_model: { m: null } } }, { result: 9, agent_id: {} }];

describe("malformed background child payloads", () => {
  for (const c of bads) {
    it(`does not throw for child = ${JSON.stringify(c)}`, () => {
      const events = [
        ev(1, "turn_started", { turn_id: "t" }),
        ev(2, "user_message", { turn_id: "t", text: "go" }),
        ev(3, "tool_call_started", { turn_id: "t", call_id: "c", tool: "delegate", input: { agent: "a", task: "x", background: true } }),
        ev(4, "background_started", { bg_id: "bg-1", call_id: "c", turn_id: "t", timeout_s: 60, ends_by_ms: 0, child: c }),
        ev(5, "tool_call_finished", { call_id: "c", ok: true, output: "session: x\nstarted", truncated: false }),
        ev(6, "turn_ended", { turn_id: "t", reason: "completed" }),
        ev(7, "background_ended", { bg_id: "bg-1", reason: "finished", ran_ms: 1, output_bytes: 0, tail: "", tail_cut: false, confirmed: "ended", child: c }),
      ];
      let err: unknown = null;
      try {
        const v = fold(events);
        sideItems(v);
        runningBackground(v);
      } catch (e) {
        err = e;
      }
      expect(err).toBeNull();
    });
  }
});

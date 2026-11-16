// A follow-up to a sub-agent (`delegate` with `session`, PLAN.md T68) is
// the same child again: the side panel lists it once, with its latest call
// and how many runs it had, never as a second sub-agent.
import { describe, expect, test } from "vitest";
import type { Item, SessionView } from "../types";
import { sideItems } from "./sideItems";

const CHILD = "01a19bf2-0000-7000-8000-00000000c0de";
const OTHER = "01a19bf2-0000-7000-8000-00000000beef";

const delegate = (call_id: string, task: string, child: string, ok = true, session?: string): Item =>
  ({
    type: "tool",
    call_id,
    author: null,
    tool: "delegate",
    input: session ? { agent: "coder", task, session } : { agent: "coder", task },
    live_output: "",
    result: { ok, output: `session: ${child}\nSub-agent @coder finished: ${ok ? "done" : "failed"}. Its own report follows.\n| ok`, exit_code: null },
    approval: null,
  }) as Item;

const view = (items: Item[]): SessionView =>
  ({
    session_id: "s",
    head_seq: 1,
    meta: null,
    items: [{ type: "user", turn_id: "t1", text: "go" }, ...items],
    running_turn: null,
    pending_approvals: [],
    unknown_events: 0,
    active_agent: null,
    running_agent: null,
    variant_selection: {},
    usage: { input_tokens: 0, output_tokens: 0, context_tokens: 0 },
    context_tokens: 0,
    checkpoints: {},
  }) as unknown as SessionView;

describe("sub-agents beside the chat", () => {
  test("a follow-up to the same child is one row with its runs and latest call", () => {
    const { agents } = sideItems(
      view([delegate("c1", "write the test", CHILD), delegate("c2", "now the error path", CHILD, false, CHILD), delegate("c3", "docs", OTHER)]),
    );
    expect(agents.map((a) => [a.child, a.runs, a.call_id, a.task, a.state])).toEqual([
      [CHILD, 2, "c2", "now the error path", "failed"],
      [OTHER, 1, "c3", "docs", "done"],
    ]);
  });

  test("a follow-up in a later turn counts the earlier runs", () => {
    const { agents } = sideItems(
      view([delegate("c1", "write the test", CHILD), { type: "turn_end", turn_id: "t1", reason: "completed" }, { type: "user", turn_id: "t2", text: "more" }, delegate("c2", "again", CHILD, true, CHILD)]),
    );
    expect(agents.map((a) => [a.call_id, a.runs])).toEqual([["c2", 2]]);
  });

  test("two different children stay two rows", () => {
    const { agents } = sideItems(view([delegate("c1", "a", CHILD), delegate("c2", "b", OTHER)]));
    expect(agents.map((a) => a.runs)).toEqual([1, 1]);
  });
});

describe("the child of a row", () => {
  test("only the node's first line names it: a uuid in an echoed refusal does not", () => {
    const refusal: Item = {
      ...(delegate("c9", "x", CHILD) as Extract<Item, { type: "tool" }>),
      result: { ok: false, output: `this step already sends a follow-up to \`x\nsession: ${OTHER}\``, exit_code: null },
    } as Item;
    const { agents } = sideItems(view([refusal]));
    expect(agents.map((a) => a.child)).toEqual([null]);
  });
});

// X.24: a sub-agent started in the background is the row of its
// `delegate` call, running (with its entry, which Stop ends) until its end,
// then its result's status; it stays listed from an earlier turn while it
// runs, and never shows among the background commands.
describe("a background sub-agent", () => {
  const started = (call_id: string): Item =>
    ({
      ...(delegate(call_id, "long work", CHILD) as Extract<Item, { type: "tool" }>),
      input: { agent: "coder", task: "long work", background: true },
      result: { ok: true, output: `session: ${CHILD}\nSub-agent @coder started in the background as bg-1.`, exit_code: null },
    }) as Item;
  const bg = (ended: boolean) => ({ "bg-1": { call_id: "c1", ends_by_ms: 1, timeout_s: 1800, ended, child: { session: CHILD, agent_id: "coder" } } });

  test("runs until its end, from an earlier turn too, and is no command", () => {
    const v = view([started("c1"), { type: "turn_end", turn_id: "t1", reason: "completed" }, { type: "user", turn_id: "t2", text: "more" }]);
    v.background = bg(false);
    const { agents, commands } = sideItems(v);
    expect(agents.map((a) => [a.call_id, a.state, a.bg_id])).toEqual([["c1", "running", "bg-1"]]);
    expect(commands).toEqual([]);
  });

  test("then shows its result's status, or stopped", () => {
    for (const [reason, tail, want] of [
      ["finished", `session: ${CHILD}\nSub-agent @coder finished: partial. Its own report follows.\n| ok`, "partial"],
      ["stopped_by_user", `session: ${CHILD}\nSub-agent @coder finished: failed.`, "stopped"],
    ] as const) {
      const v = view([started("c1"), { type: "background_ended", bg_id: "bg-1", reason, exit_code: null, ran_ms: 5, tail, agent: "coder" }]);
      v.background = bg(true);
      const { agents } = sideItems(v);
      expect(agents.map((a) => [a.state, a.bg_id])).toEqual([[want, undefined]]);
    }
  });
});

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

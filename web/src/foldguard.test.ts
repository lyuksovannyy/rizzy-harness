// Nothing a node sends can crash the fold (review of PRs 130, 134, 137: three
// node payloads of the wrong shape crashed a fold or a row). The guard is one
// step where events enter the fold: every payload is checked against what the
// fold reads, and an event whose required fields are missing or of the wrong
// type is dropped and counted in `unknown_events`, as the Rust fold counts an
// event serde cannot read. An optional field of the wrong type is left out.
//
// The property: for every event kind of the shared fixture and every field in
// turn (nested ones too) replaced by a bad value, the fold never throws, and
// either the event is dropped (the state is the state before it, with only
// the counter and the cursor moved) or the state it gives is well typed,
// checked here against types.ts independently of the guard's own table. Rows
// render the fold's items, so a well-typed state is what keeps them whole.
import { afterEach, describe, expect, it, vi } from "vitest";
import { readFileSync } from "node:fs";
import { apply, emptyView, fold } from "./fold";
import type { RawEvent, SessionView } from "./types";

const fixture = JSON.parse(readFileSync(new URL("./__fixtures__/session.json", import.meta.url), "utf8"));
const EVENTS = fixture.events as RawEvent[];
const ev = (seq: number, kind: string, payload: unknown): RawEvent => ({ session_id: "s", seq, ts_ms: 0, kind, payload }) as RawEvent;

afterEach(() => vi.restoreAllMocks());

// ── A checker of the view's types (types.ts), written apart from the guard ──

type Check = (x: unknown, at: string) => void;
const fail = (at: string, x: unknown) => {
  throw new Error(`${at}: ${JSON.stringify(x)?.slice(0, 80)}`);
};
const isRec = (x: unknown): x is Record<string, unknown> => !!x && typeof x == "object" && !Array.isArray(x);
const S: Check = (x, at) => void (typeof x == "string" || fail(at, x));
const N: Check = (x, at) => void ((typeof x == "number" && Number.isFinite(x)) || fail(at, x));
const B: Check = (x, at) => void (typeof x == "boolean" || fail(at, x));
const TRUE: Check = (x, at) => void (x === true || fail(at, x));
const ANY: Check = () => {};
const OBJ: Check = (x, at) => void (isRec(x) || fail(at, x));
const nul = (c: Check): Check => (x, at) => (x === null ? undefined : c(x, at));
const arr = (c: Check): Check => (x, at) => {
  if (!Array.isArray(x)) fail(at, x);
  (x as unknown[]).forEach((y, i) => c(y, `${at}[${i}]`));
};
const map = (c: Check): Check => (x, at) => {
  OBJ(x, at);
  for (const k of Object.keys(x as object)) c((x as Record<string, unknown>)[k], `${at}.${k}`);
};
/** Required keys, then optional ones (`?key`: absent, never undefined). */
const rec = (shape: Record<string, Check>): Check => (x, at) => {
  OBJ(x, at);
  const o = x as Record<string, unknown>;
  for (const [k0, c] of Object.entries(shape)) {
    const opt = k0.startsWith("?");
    const k = opt ? k0.slice(1) : k0;
    if (!Object.hasOwn(o, k)) {
      if (!opt) fail(`${at}.${k} missing`, o);
      continue;
    }
    if (opt && o[k] === undefined) fail(`${at}.${k} undefined`, o);
    c(o[k], `${at}.${k}`);
  }
};
const tokens = rec({ input_tokens: N, output_tokens: N, "?cache_read_tokens": N, "?cache_write_tokens": N });
const usage = rec({
  input_tokens: N,
  output_tokens: N,
  "?context_tokens": N,
  "?cache_read_tokens": N,
  "?cache_write_tokens": N,
  "?window": N,
  "?summary_at": N,
  "?model": S,
  "?by_model": map(tokens),
});
const agentRef = rec({ agent_id: S, name: S, prompt_hash: S });
const staged = rec({
  stage_id: S,
  "?call_id": nul(S),
  by: S,
  name: S,
  filename: S,
  packed: nul(S),
  size: N,
  sha256: S,
  "?kind_hint": nul(S),
  source_path: S,
  "?notes": nul(S),
  meta: OBJ,
});
const ITEM: Record<string, Check> = {
  user: rec({
    type: S,
    turn_id: S,
    text: S,
    "?attachments": arr(S),
    "?files": arr(S),
    "?reply": rec({ message_id: S, block_id: S, values: OBJ }),
    "?images": arr(rec({ media_id: S, sha256: S, mime: S, name: S, width: N, height: N })),
    "?queued": rec({ seq: N, read: B }),
    "?triggers": arr(S),
    "?tags": arr(S),
  }),
  assistant: rec({
    type: S,
    message_id: S,
    text: S,
    done: B,
    author: nul(S),
    variant: nul(rec({ group: S, index: N, "?model": S })),
    "?blocks": arr(rec({ ordinal: N, hash: S, id: S, type: S, "?spec": ANY, "?error": S })),
  }),
  thinking: rec({ type: S, message_id: S, blocks: arr(rec({ type: S, "?text": S })) }),
  tool: rec({
    type: S,
    call_id: S,
    author: nul(S),
    tool: S,
    input: ANY,
    live_output: S,
    result: nul(
      rec({ ok: B, output: S, exit_code: nul(N), "?sources": arr(rec({ id: S, url: S, title: S })) }),
    ),
    approval: nul(
      rec({
        approval_id: S,
        reason: S,
        "?turn_allow": TRUE,
        "?chat_allow": TRUE,
        decision: nul(S),
        by: nul(S),
        "?note": S,
      }),
    ),
    "?staged": staged,
  }),
  error: rec({ type: S, message: S }),
  turn_end: rec({ type: S, turn_id: S, reason: S }),
  agent_changed: rec({ type: S, agent: agentRef, "?model": nul(S) }),
  approval_mode_changed: rec({ type: S, mode: S, "?requested": S }),
  step_limit_changed: rec({ type: S, max_steps: N, "?requested": N }),
  summary: rec({ type: S, through_seq: N, text: S, "?manual": TRUE, "?focus": S }),
  shortened: rec({
    type: S,
    turn_id: S,
    cleared: B,
    "?steps": N,
    "?text": S,
    "?thinking": TRUE,
    tokens_before: N,
    tokens_after: N,
  }),
  fallback: rec({ type: S, turn_id: S, model: S, from: S, reason: nul(S) }),
  artifact: rec({ type: S, staged }),
  artifact_failed: rec({ type: S, path: S, name: nul(S), error: S }),
  background_ended: rec({ type: S, bg_id: S, reason: S, exit_code: nul(N), ran_ms: N, tail: S, "?agent": S }),
  callback: rec({ type: S, turn_id: S, bg_ids: arr(S) }),
  called: rec({ type: S, turn_id: S, agent: nul(agentRef), message_seq: N }),
  peers_skipped: rec({ type: S, agents: arr(S), reason: S }),
  retry: rec({ type: S, turn_id: S, of: S, reason: S, "?failed_model": S, "?model": S }),
  callback_refused: rec({ type: S, bg_ids: arr(S), reason: S }),
  handoff: rec({ type: S, through_seq: N, "?text": S, "?model": S, "?failed": arr(rec({ model: S, reason: S })) }),
};
const item: Check = (x, at) => {
  OBJ(x, at);
  const c = ITEM[(x as { type: string }).type];
  if (!c) fail(`${at}.type`, x);
  // A rewound item (X.45) carries the seq of the rewind, whatever it is.
  const { rewound, ...rest } = x as { rewound?: unknown };
  if (rewound !== undefined) N(rewound, `${at}.rewound`);
  c!(rest, at);
};
const viewCheck = rec({
  session_id: nul(S),
  head_seq: N,
  meta: nul(
    rec({
      title: S,
      cwd: S,
      model: S,
      backend: S,
      node_id: S,
      approval_mode: S,
      profile: S,
      "?agent_id": S,
      "?incognito": TRUE,
      "?parent_session": S,
      "?no_callbacks": TRUE,
      "?exec_mode": S,
    }),
  ),
  items: arr(item),
  running_turn: nul(S),
  pending_approvals: arr(S),
  unknown_events: N,
  running_agent: nul(agentRef),
  variant_selection: map(S),
  usage,
  context_tokens: N,
  checkpoints: map(rec({ before: nul(S), after: nul(S), files: arr(S), restored: B })),
  "?background": map(
    rec({ call_id: S, ends_by_ms: N, timeout_s: N, ended: B, "?child": rec({ session: S, agent_id: S }) }),
  ),
  "?todo": arr(rec({ content: S, status: S })),
  "?max_steps": N,
  "?retry": rec({ turn_id: S, model: S, attempt: N, of: N, at_ms: N, wait_ms: N, reason: S }),
  "?context_window": N,
  "?summary_at": N,
  "?context_model": S,
  "?chat_allows": map(rec({ tool: S, input: ANY })),
  "?turn_usage": usage,
  "?follow_ups": arr(S),
  "?rewinds": arr(
    rec({ seq: N, message_seq: N, turn_id: S, turns: N, files: B, undone: B, can_undo: B }),
  ),
});
const wellTyped = (v: SessionView) => viewCheck(v, "view");

// ── The bad values and the fields they replace ──

const deep = (n: number): unknown => (n === 0 ? "bottom" : { a: [deep(n - 1)] });
const BAD: [string, unknown][] = [
  ["missing", undefined],
  ["null", null],
  ["a number", 7],
  ["a string", "x"],
  ["an array", [1, "two", null]],
  ["an object", { a: 1 }],
  ["deeply nested", deep(200)],
  ["a very long string", "y".repeat(200_000)],
];

/** Every path into a payload, to depth 3, through objects and an array's first element. */
function paths(x: unknown, depth = 3, at: (string | number)[] = []): (string | number)[][] {
  if (depth === 0) return [];
  const out: (string | number)[][] = [];
  if (Array.isArray(x)) {
    if (x.length > 0) out.push([...at, 0], ...paths(x[0], depth - 1, [...at, 0]));
  } else if (isRec(x)) {
    for (const k of Object.keys(x)) out.push([...at, k], ...paths(x[k], depth - 1, [...at, k]));
  }
  return out;
}
function replaced(payload: unknown, path: (string | number)[], value: unknown): unknown {
  const p = structuredClone(payload) as Record<string | number, unknown>;
  let o = p;
  for (const k of path.slice(0, -1)) o = o[k] as Record<string | number, unknown>;
  const last = path.at(-1)!;
  if (value === undefined) {
    if (Array.isArray(o)) o.splice(last as number, 1);
    else delete o[last];
  } else o[last] = value;
  return p;
}
/** The state a dropped event leaves: the one before, its cursor and counter moved. */
const droppedFrom = (before: SessionView, e: RawEvent): SessionView => ({
  ...before,
  session_id: e.session_id,
  head_seq: e.seq,
  unknown_events: before.unknown_events + 1,
});

const KINDS = [...new Set(EVENTS.map((e) => e.kind))].filter((k) => k !== "from_a_newer_node");
// One event of each kind (its first), with the fixture's events before it.
const CASES = KINDS.map((kind) => {
  const i = EVENTS.findIndex((e) => e.kind === kind);
  return { kind, i, before: fold(EVENTS.slice(0, i)), e: EVENTS[i]! };
});

describe("the fold guard: no payload breaks the fold", () => {
  it("the fixture covers the kinds the fold handles, and folds to a well-typed state", () => {
    expect(KINDS.length).toBeGreaterThanOrEqual(34);
    wellTyped(fold(EVENTS));
  });

  for (const { kind, i, before, e } of CASES) {
    it(`${kind}: every field replaced by a bad value is dropped or folds well typed`, () => {
      vi.spyOn(console, "warn").mockImplementation(() => {});
      const rest = EVENTS.slice(i + 1);
      let kept = 0;
      let dropped = 0;
      const targets: [(string | number)[] | null, string, unknown][] = [
        // The whole payload, then each field in turn.
        ...BAD.map(([name, value]) => [null, name, value] as [null, string, unknown]),
        ...paths(e.payload).flatMap((path) => BAD.map(([name, value]) => [path, name, value] as [(string | number)[], string, unknown])),
      ];
      for (const [path, name, value] of targets) {
        const what = `${kind} ${path ? path.join(".") : "payload"} = ${name}`;
        const v = structuredClone(before);
        const bad = { ...e, payload: path ? replaced(e.payload, path, value) : value };
        expect(() => apply(v, bad), what).not.toThrow();
        if (JSON.stringify(v) === JSON.stringify(droppedFrom(before, e))) {
          expect(v, what).toStrictEqual(droppedFrom(before, e));
          dropped++;
        } else {
          expect(v.unknown_events, what).toBe(before.unknown_events);
          expect(() => wellTyped(v), what).not.toThrow();
          kept++;
        }
        // And the rest of the log still folds on top of it, well typed.
        expect(() => {
          for (const r of rest) apply(v, r);
          wellTyped(v);
        }, what).not.toThrow();
      }
      expect(dropped + kept).toBe(targets.length);
      // A payload that is not an object is never folded.
      expect(dropped).toBeGreaterThanOrEqual(BAD.filter(([, x]) => !isRec(x)).length);
    });
  }

  it("retry_given_up (not in the fixture) with a bad payload is dropped", () => {
    const v = fold([ev(1, "turn_started", { turn_id: "t" })]);
    vi.spyOn(console, "warn").mockImplementation(() => {});
    const before = structuredClone(v);
    apply(v, ev(2, "retry_given_up", 5));
    expect(v).toStrictEqual(droppedFrom(before, ev(2, "x", null)));
  });

  it("a field of the wrong type in a required place drops the event, a person's text never shows as a number", () => {
    vi.spyOn(console, "warn").mockImplementation(() => {});
    const v = fold([ev(1, "turn_started", { turn_id: "t" }), ev(2, "user_message", { turn_id: "t", text: 42 })]);
    expect(v.items).toEqual([]);
    expect(v.unknown_events).toBe(1);
  });
});

describe("the fold guard: what a dropped event leaves", () => {
  it("warns once, with the kind and the sequence number only, never the content", () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    const v = fold([ev(1, "turn_started", { turn_id: "t" })]);
    apply(v, ev(2, "user_message", { turn_id: "t", text: { secret: "sk-SECRET-VALUE" } }));
    expect(warn).toHaveBeenCalledTimes(1);
    const said = JSON.stringify(warn.mock.calls[0]);
    expect(said).toContain("user_message");
    expect(said).toContain("2");
    expect(said).not.toContain("SECRET");
  });

  it("an unknown kind is counted and warned about, and changes nothing else", () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    const v = fold([ev(1, "turn_started", { turn_id: "t" })]);
    const before = structuredClone(v);
    apply(v, ev(2, "from_a_newer_node", { anything: true }));
    expect(v).toStrictEqual(droppedFrom(before, ev(2, "x", null)));
    expect(warn).toHaveBeenCalledTimes(1);
  });

  it("a hostile kind name is cut in the warning", () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    fold([ev(1, "k".repeat(10_000), {})]);
    expect(JSON.stringify(warn.mock.calls[0]).length).toBeLessThan(400);
  });

  it("a known kind with extra fields still folds, the same as without them", () => {
    const extra = EVENTS.map((e) => (isRec(e.payload) ? { ...e, payload: { ...e.payload, from_the_future: { x: [1] }, zz: "new" } } : e));
    expect(fold(extra)).toStrictEqual(fixture.expected);
  });

  it("is a pure function of the log: the same events, dropped ones too, give the same state", () => {
    vi.spyOn(console, "warn").mockImplementation(() => {});
    const log = [
      ...EVENTS.slice(0, 10),
      { ...EVENTS[10]!, payload: 5 },
      ...EVENTS.slice(11, 30),
      { ...EVENTS[30]!, payload: { turn_id: [] } },
      ...EVENTS.slice(31),
    ];
    const a = fold(log);
    expect(fold(log)).toStrictEqual(a);
    expect(a.unknown_events).toBe(fixture.expected.unknown_events + 2);
  });

  it("node ids never reach an object's prototype (checkpoint, background, allow, variant)", () => {
    for (const id of ["__proto__", "constructor", "toString"]) {
      const v = fold([
        ev(1, "turn_started", { turn_id: id }),
        ev(2, "checkpoint", { turn_id: id, phase: "before", git_ref: "refs/x" }),
        ev(3, "background_started", { bg_id: id, call_id: "c", turn_id: id, timeout_s: 1, ends_by_ms: 0 }),
        ev(4, "background_ended", { bg_id: id, reason: "exited", confirmed: "ended", ran_ms: 1, output_bytes: 0 }),
        ev(5, "chat_allow_added", { grant_id: id, approval_id: "a", tool: "bash", input: {} }),
        ev(6, "variant_selected", { group: id, message_id: "m" }),
      ]);
      expect(({} as Record<string, unknown>).before, id).toBeUndefined();
      expect(({} as Record<string, unknown>).ended, id).toBeUndefined();
      for (const o of [v.checkpoints, v.background!, v.chat_allows!, v.variant_selection]) {
        expect(Object.getPrototypeOf(o), id).toBe(Object.prototype);
        expect(Object.hasOwn(o, id), id).toBe(true);
      }
      expect(v.checkpoints[id]).toStrictEqual({ before: "refs/x", after: null, files: [], restored: false });
      expect(v.background![id]!.ended).toBe(true);
      wellTyped(v);
      // And they serialise as the Rust view writes the map: an own key.
      expect(Object.keys(JSON.parse(JSON.stringify(v.checkpoints)))).toEqual([id]);
    }
  });
});

describe("the three crashes found in review, as regressions", () => {
  it("PR 130: images_withheld whose calls is not a list", () => {
    vi.spyOn(console, "warn").mockImplementation(() => {});
    const base = () =>
      fold([
        ev(1, "turn_started", { turn_id: "t" }),
        ev(2, "user_message", { turn_id: "t", text: "go" }),
        ev(3, "tool_call_started", { turn_id: "t", call_id: "c", tool: "read", input: {} }),
        ev(4, "tool_call_finished", { call_id: "c", ok: true, output: "x", truncated: false }),
      ]);
    const plain = JSON.stringify(base().items);
    for (const calls of ["c", 7, { a: 1 }, null]) {
      const v = base();
      expect(() => apply(v, ev(5, "images_withheld", { turn_id: "t", calls, status: 400 }))).not.toThrow();
      expect(JSON.stringify(v.items)).toBe(plain);
      wellTyped(v);
    }
  });

  // PR 134 review F7: a background sub-agent's `child` of the wrong shape.
  const children: unknown[] = [null, 5, "x", [], { session: 7 }, { usage: 5 }, { usage: "x" }, { usage: { by_model: "abc" } }, { usage: { by_model: { m: null } } }, { result: 9, agent_id: {} }];
  for (const c of children) {
    it(`PR 134: background child = ${JSON.stringify(c)}`, () => {
      vi.spyOn(console, "warn").mockImplementation(() => {});
      const v = fold([
        ev(1, "turn_started", { turn_id: "t" }),
        ev(2, "user_message", { turn_id: "t", text: "go" }),
        ev(3, "tool_call_started", { turn_id: "t", call_id: "c", tool: "delegate", input: { agent: "a", task: "x", background: true } }),
        ev(4, "background_started", { bg_id: "bg-1", call_id: "c", turn_id: "t", timeout_s: 60, ends_by_ms: 0, child: c }),
        ev(5, "tool_call_finished", { call_id: "c", ok: true, output: "session: x\nstarted", truncated: false }),
        ev(6, "turn_ended", { turn_id: "t", reason: "completed" }),
        ev(7, "background_ended", { bg_id: "bg-1", reason: "finished", ran_ms: 1, output_bytes: 0, tail: "", tail_cut: false, confirmed: "ended", child: c }),
      ]);
      wellTyped(v);
      // The command's start and end still show; the bad child is left out.
      expect(v.background?.["bg-1"]?.ended).toBe(true);
      expect(v.items.some((it) => it.type === "background_ended")).toBe(true);
    });
  }

  // PR 137: a node's malformed thinking crashed the chat page.
  const thinkings: [string, unknown][] = [
    ["a number", 5],
    ["a string", "x"],
    ["blocks not an array", { blocks: "x" }],
    ["a text that is a number", { blocks: [{ type: "signed", text: 7, signature: "S" }] }],
    ["a null block", { blocks: [null] }],
    ["a number block", { blocks: [3] }],
    ["a block without a type", { blocks: [{ text: "t" }, { type: "text", text: "kept" }] }],
  ];
  for (const [name, thinking] of thinkings) {
    it(`PR 137: thinking ${name}: the message still shows`, () => {
      vi.spyOn(console, "warn").mockImplementation(() => {});
      const v = emptyView();
      apply(v, { session_id: "s", seq: 1, ts_ms: 0, kind: "assistant_message", payload: { turn_id: "t", message_id: "m1", text: "hi", tool_calls: [], thinking } });
      wellTyped(v);
      expect(v.items.find((it) => it.type === "assistant")).toMatchObject({ text: "hi" });
    });
  }
});

describe("the fold guard: enum values (review of PR 156, MEDIUM 1)", () => {
  // A value outside its enum is what serde refuses, so the event is dropped
  // and counted, as in the Rust fold; rows key tables by these strings.
  const cases: [string, Record<string, unknown>, string][] = [
    ["session_created", { title: "t", cwd: "/w", model: "m", backend: "b", node_id: "n", approval_mode: "ask", profile: "agentic" }, "exec_mode"],
    ["session_created", { title: "t", cwd: "/w", model: "m", backend: "b", node_id: "n", approval_mode: "ask", profile: "agentic" }, "approval_mode"],
    ["approval_mode_changed", { mode: "ask" }, "mode"],
    ["turn_ended", { turn_id: "t", reason: "completed" }, "reason"],
    ["checkpoint", { turn_id: "t", phase: "before", git_ref: "r" }, "phase"],
  ];
  for (const [kind, payload, field] of cases)
    for (const bad of ["__proto__", "constructor", "a_value_from_a_newer_node", ""]) {
      it(`${kind}.${field} = ${JSON.stringify(bad)} is dropped and counted`, () => {
        vi.spyOn(console, "warn").mockImplementation(() => {});
        const v = fold([ev(1, "turn_started", { turn_id: "t0" })]);
        const before = structuredClone(v);
        apply(v, ev(2, kind, { ...payload, [field]: bad }));
        expect(v).toStrictEqual(droppedFrom(before, ev(2, kind, null)));
        // And the good value folds.
        const w = fold([ev(1, "turn_started", { turn_id: "t0" })]);
        apply(w, ev(2, kind, payload));
        expect(w.unknown_events).toBe(0);
      });
    }

  it("an enum value outside its list in an array element drops the event too", () => {
    vi.spyOn(console, "warn").mockImplementation(() => {});
    const v = fold([
      ev(1, "todo_updated", { turn_id: "t", call_id: "c", items: [{ content: "a", status: "__proto__" }, { content: "b", status: "done" }] }),
    ]);
    expect(v.todo).toBeUndefined();
    expect(v.unknown_events).toBe(1);
  });
});

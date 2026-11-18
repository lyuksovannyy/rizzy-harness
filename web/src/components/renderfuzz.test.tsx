// @vitest-environment happy-dom
// Nothing a node or a model sends takes down a chat (review of PR 156, the
// reviewer's proof made permanent): logs folded and RENDERED through the
// conversation. The named cases are the review's findings; the fuzz replaces
// every string and number leaf of every fixture event, one at a time, with
// prototype names and odd numbers, folds the whole log and renders it. A
// pass means no screen boundary and no row boundary showed.
import { act } from "react";
import { readFileSync } from "node:fs";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { fold } from "../fold";
import { fields } from "../lib/callText";
import type { Live } from "../live";
import type { Features, RawEvent, SessionView } from "../types";

vi.mock("../api", () => ({
  api: { features: () => Promise.resolve({ artifacts: { enabled: true, show: true, why: null }, media: { enabled: false, show: false, vision_images: false }, routines: { enabled: true } } as unknown as Features), prompt: vi.fn() },
  call: vi.fn(() => Promise.resolve({ ok: true })),
  HttpError: Error,
}));
vi.mock("../apiPages", () => ({
  pageApi: { deleteSession: vi.fn(), approve: vi.fn(), turnDiff: vi.fn(() => Promise.resolve({ files: [] })), restoreTurn: vi.fn(), retryTurn: vi.fn(), setAgent: vi.fn() },
}));
const artifactsApi = { forSession: vi.fn(() => Promise.resolve([])), save: vi.fn(() => Promise.resolve({ requested: true })) };
vi.mock("../lib/artifacts", async (original) => ({ ...(await original<typeof import("../lib/artifacts")>()), artifactsApi }));
const mediaApi = { list: vi.fn(() => Promise.resolve([])), get: vi.fn() };
vi.mock("../lib/media", async (original) => ({ ...(await original<typeof import("../lib/media")>()), mediaApi }));
const { default: Conversation } = await import("./Conversation");
const { ScreenBoundary } = await import("./ScreenBoundary");
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const fixture = JSON.parse(readFileSync(process.cwd() + "/src/__fixtures__/session.json", "utf8"));
const EVENTS = fixture.events as RawEvent[];
const SID = EVENTS[0]!.session_id;

let host: HTMLElement;
let root: Root;
let current: SessionView;
const live = { watch: vi.fn(), unwatch: vi.fn(), view: () => current, onTopic: vi.fn(() => () => {}) } as unknown as Live;
let errors: string[] = [];
beforeEach(() => {
  errors = [];
  vi.spyOn(console, "error").mockImplementation((...a: unknown[]) => void errors.push(a.map((e) => (e instanceof Error ? `${e.message} @ ${e.stack?.split("\n").slice(1, 3).join(" ")}` : String(e))).join(" ")));
  vi.spyOn(console, "warn").mockImplementation(() => {});
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
  vi.restoreAllMocks();
});

/** Fold the log, render the chat inside the PR's boundary; the boundary's text if it showed. */
async function render(events: RawEvent[]): Promise<string | null> {
  current = fold(events);
  await act(async () =>
    root.render(
      <ScreenBoundary>
        <Conversation live={live} sessionId={SID} row={null} nodes={[]} agents={[]} models={[]} />
      </ScreenBoundary>,
    ),
  );
  await act(() => new Promise((r) => setTimeout(r, 0)));
  const alert = host.querySelector("[role=alert], [data-testid=row-failed]");
  return alert ? `${alert.textContent} | ${errors.find((e) => !e.includes("act(") && !e.startsWith("%o")) ?? errors.join(" // ").slice(0, 400) ?? ""}` : null;
}
const last = EVENTS.at(-1)!;
const ev = (i: number, kind: string, payload: object): RawEvent => ({ session_id: SID, seq: last.seq + i, ts_ms: 0, kind, payload }) as RawEvent;

describe("well-folded states that broke a row (review of PR 156)", () => {
  test("baseline: the fixture renders", async () => {
    expect(await render(EVENTS)).toBeNull();
  });
  for (const id of ["__proto__", "constructor", "toString"]) {
    test(`turn id ${id} after an earlier turn end`, async () => {
      const out = await render([...EVENTS, ev(1, "turn_started", { turn_id: id }), ev(2, "turn_ended", { turn_id: id, reason: "completed" })]);
      expect(out).toBeNull();
    });
  }
  for (const tool of ["constructor", "toString", "hasOwnProperty", "__proto__", "valueOf"]) {
    test(`tool named ${tool} (a model's call of an unknown tool)`, async () => {
      let direct = "ok";
      try {
        fields(tool, { a: 1 });
      } catch (e) {
        direct = String(e);
      }
      const out = await render([...EVENTS, ev(1, "turn_started", { turn_id: "tz" }), ev(2, "tool_call_started", { turn_id: "tz", call_id: "cz", tool, input: { a: 1 } })]);
      expect(direct).toBe("ok");
      expect(out).toBeNull();
    });
  }
  test("variant group constructor", async () => {
    const out = await render([
      ...EVENTS,
      ev(1, "turn_started", { turn_id: "tv" }),
      ev(2, "assistant_message", { turn_id: "tv", message_id: "va", text: "A", variant: { group: "constructor", index: 0 } }),
      ev(3, "assistant_message", { turn_id: "tv", message_id: "vb", text: "B", variant: { group: "constructor", index: 1 } }),
    ]);
    expect(host.textContent).toContain("A");
    expect(out).toBeNull();
  });
});

// Every leaf of every fixture event, one at a time, replaced; the whole log folded and rendered.
const STRS = ["__proto__", "constructor", "toString", "hasOwnProperty", "then", "length", ""];
const NUMS = [-1, 1.5, -0, 1e308, 2 ** 53 + 1, 0];
type P = (string | number)[];
function leaves(x: unknown, at: P = [], depth = 4): [P, unknown][] {
  if (depth === 0) return [];
  if (Array.isArray(x)) return x.flatMap((y, i) => (i < 2 ? leaves(y, [...at, i], depth - 1) : []));
  if (x && typeof x == "object") return Object.entries(x).flatMap(([k, y]) => leaves(y, [...at, k], depth - 1));
  return [[at, x]];
}
function set(payload: unknown, path: P, v: unknown) {
  const p = structuredClone(payload) as Record<string | number, unknown>;
  let o = p;
  for (const k of path.slice(0, -1)) o = o[k] as Record<string | number, unknown>;
  o[path.at(-1)!] = v;
  return p;
}
describe("fuzzed logs, rendered", () => {
  test(
    "no fuzzed log makes the chat screen fail",
    async () => {
      const failures = new Map<string, string>();
      let n = 0;
      for (let i = 0; i < EVENTS.length; i++) {
        const e = EVENTS[i]!;
        for (const [path, val] of leaves(e.payload)) {
          const vals = typeof val == "string" ? STRS : typeof val == "number" ? NUMS : [];
          for (const v of vals) {
            const log = EVENTS.slice();
            log[i] = { ...e, payload: set(e.payload, path, v) } as RawEvent;
            let out: string | null;
            try {
              out = await render(log);
            } catch (err) {
              out = `THREW ${String(err)}`;
            }
            n++;
            if (out) {
              const key = `${e.kind}.${path.join(".")}=${JSON.stringify(v)}`;
              failures.set(key, out.slice(0, 200));
              // Fresh root after a failure.
              act(() => root.unmount());
              root = createRoot(host);
            }
          }
        }
      }
      expect(n).toBeGreaterThan(2000);
      expect([...failures.entries()].map(([k, v]) => `${k} -> ${v}`)).toEqual([]);
    },
    600_000,
  );
});

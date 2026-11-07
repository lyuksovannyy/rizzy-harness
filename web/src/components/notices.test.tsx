// @vitest-environment happy-dom
// Notices about models gone from their provider: what an admin reads (and
// the way to fix it) against what anyone else reads, and the fallback event
// that says another model answered.
import { act } from "react";
import { createRoot } from "react-dom/client";
import { describe, expect, test } from "vitest";
import { fold } from "../fold";
import { fallbackNote, rawModel } from "../lib/models";
import type { Notice, RawEvent } from "../types";
import { FIX_DEFAULT, FIX_PROVIDERS, Notices, noticeText } from "./Notices";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const agents = [
  { id: "general", name: "General" },
  { id: "coder", name: "Coder" },
];
const notice = (over: Partial<Notice> = {}): Notice => ({
  id: "n1",
  kind: "model_unavailable",
  model: "openai::gpt-old",
  provider: "OpenAI",
  agents: ["general", "coder"],
  your_default: false,
  global_default: false,
  can_fix: false,
  ...over,
});

describe("noticeText", () => {
  test("names the model, its provider and the agents using it", () => {
    expect(noticeText(notice(), agents).text).toBe("Model gpt-old (OpenAI) is no longer available — used by General, Coder.");
    expect(noticeText(notice({ agents: ["ghost"] }), agents).text).toContain("used by ghost.");
    expect(noticeText(notice({ agents: [], global_default: true, your_default: true }), agents).text).toBe(
      "Model gpt-old (OpenAI) is no longer available. It is the default model for everyone and your default model.",
    );
  });

  test("an admin is sent to the providers to fix it", () => {
    const r = noticeText(notice({ can_fix: true, your_default: true }), agents);
    expect(r.link).toEqual({ href: FIX_PROVIDERS, label: "Fix in Settings" });
    expect(r.hint).toMatch(/Pick another model/);
  });

  test("anyone else is told, and can only change their own default", () => {
    const mine = noticeText(notice({ your_default: true }), agents);
    expect(mine.link).toEqual({ href: FIX_DEFAULT, label: "Change your default" });
    const theirs = noticeText(notice(), agents);
    expect(theirs.link).toBeNull();
    expect(theirs.hint).toMatch(/An admin needs to replace it/);
  });
});

describe("Notices", () => {
  test("one per notice, a fix link only for admins, nothing to dismiss", () => {
    const host = document.createElement("div");
    const root = createRoot(host);
    act(() =>
      root.render(
        <Notices
          notices={[notice({ id: "a", can_fix: true }), notice({ id: "b", model: "x::y" })]}
          agents={agents.map((a) => ({ ...a, description: "", builtin: true, default_model: null, fallbacks: [], unavailable: [] }))}
        />,
      ),
    );
    const items = host.querySelectorAll(".notice");
    expect(items).toHaveLength(2);
    expect(items[0]!.querySelector("a")!.getAttribute("href")).toBe(FIX_PROVIDERS);
    expect(items[1]!.querySelector("a")).toBeNull();
    expect(host.querySelector("button")).toBeNull();
    act(() => root.render(<Notices notices={[]} agents={[]} />));
    expect(host.innerHTML).toBe("");
    act(() => root.unmount());
  });
});

describe("model_fallback", () => {
  test("folds into a fallback item of its turn", () => {
    const ev = (seq: number, kind: string, payload: unknown): RawEvent => ({ session_id: "s", seq, ts_ms: seq, kind, payload });
    const v = fold([
      ev(1, "turn_started", { turn_id: "t", agent: null }),
      ev(2, "model_fallback", { turn_id: "t", model: "anthropic::claude-y", from: "openai::gpt-old" }),
      ev(3, "model_fallback", { turn_id: "t", model: "a::b", reason: "429 Too Many Requests" }),
    ]);
    expect(v.items).toEqual([
      { type: "fallback", turn_id: "t", model: "anthropic::claude-y", from: "openai::gpt-old", reason: null },
      { type: "fallback", turn_id: "t", model: "a::b", from: null, reason: "429 Too Many Requests" },
    ]);
    expect(v.unknown_events).toBe(0);
  });
});

describe("the fallback note", () => {
  test("says why: gone, or failed with the provider's error on hover", () => {
    expect(fallbackNote({ model: "a::b", from: "openai::gpt-old", reason: "unavailable" })).toEqual({
      text: "Answered by b — the main model was unavailable",
      title: "Main model: gpt-old",
    });
    expect(fallbackNote({ model: "a::b", from: null })).toEqual({ text: "Answered by b — the main model was unavailable", title: "" });
    const failed = fallbackNote({ model: "a::b", from: "gpt-4o", reason: "429 Too Many Requests" });
    expect(failed.text).toBe("Answered by b — the main model failed");
    // A legacy unprefixed id keeps its first letter.
    expect(failed.title).toBe("Main model: gpt-4o\n429 Too Many Requests");
  });

  test("rawModel leaves an unprefixed id whole", () => {
    expect(rawModel("gpt-4o")).toBe("gpt-4o");
    expect(rawModel("a::b")).toBe("b");
    expect(rawModel("a::")).toBe("a::");
  });
});

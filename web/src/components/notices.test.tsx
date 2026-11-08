// @vitest-environment happy-dom
// Notices about models gone from their provider: what a server admin reads
// (and the way to fix it), what a workspace admin reads about the workspace's
// own settings, against what anyone else reads, and the fallback event that
// says another model answered.
import { act } from "react";
import { createRoot } from "react-dom/client";
import { describe, expect, test } from "vitest";
import { fold } from "../fold";
import { fallbackNote, rawModel } from "../lib/models";
import type { Notice, RawEvent } from "../types";
import { FIX_DEFAULT, FIX_PROVIDERS, FIX_WORKSPACE, Notices, noticeText } from "./Notices";

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
  workspace_default: false,
  can_fix: false,
  workspace: false,
  can_fix_workspace: false,
  ...over,
});

describe("noticeText", () => {
  test("names the model, its provider and the agents using it", () => {
    expect(noticeText(notice(), agents).text).toBe("Model gpt-old (OpenAI) is no longer available — used by General, Coder.");
    expect(noticeText(notice({ agents: ["ghost"] }), agents).text).toContain("used by ghost.");
    expect(noticeText(notice({ agents: [], global_default: true, your_default: true }), agents).text).toBe(
      "Model gpt-old (OpenAI) is no longer available. It is the server default model and your default model.",
    );
    expect(noticeText(notice({ agents: [], workspace_default: true, workspace: true }), agents).text).toBe(
      "Model gpt-old (OpenAI) is no longer available. It is this workspace's default model.",
    );
    // The server default, also named by one of the workspace's chains, is still the server's.
    expect(noticeText(notice({ agents: ["coder"], global_default: true, workspace: true }), agents).text).toBe(
      "Model gpt-old (OpenAI) is no longer available — used by Coder. It is the server default model.",
    );
  });

  test("the links point at the new settings paths", () => {
    expect([FIX_PROVIDERS, FIX_DEFAULT, FIX_WORKSPACE]).toEqual([
      "#/settings/server/providers",
      "#/settings/account/preferences",
      "#/settings/workspace-admin/models",
    ]);
  });

  test("a server admin is sent to the providers to fix it", () => {
    const r = noticeText(notice({ can_fix: true, your_default: true }), agents);
    expect(r.links).toEqual([{ href: FIX_PROVIDERS, label: "Fix in Server settings" }]);
    expect(r.hint).toMatch(/Pick another model/);
  });

  test("anyone else is told, and can only change their own default", () => {
    const mine = noticeText(notice({ your_default: true }), agents);
    expect(mine.links).toEqual([{ href: FIX_DEFAULT, label: "Change your default" }]);
    const theirs = noticeText(notice(), agents);
    expect(theirs.links).toEqual([]);
    expect(theirs.hint).toMatch(/A server admin needs to replace it/);
  });

  test("a use from the workspace's settings is fixed there, by its admins", () => {
    const admin = noticeText(notice({ workspace: true, can_fix_workspace: true }), agents);
    expect(admin.links).toEqual([{ href: FIX_WORKSPACE, label: "Fix in Workspace admin" }]);
    // A server admin who cannot change the workspace is not sent to the providers.
    const server = noticeText(notice({ workspace: true, can_fix: true }), agents);
    expect(server.links).toEqual([]);
    expect(server.hint).toMatch(/A workspace admin needs to replace it/);
    // Nor for the workspace's own default.
    expect(noticeText(notice({ workspace: true, workspace_default: true, can_fix: true }), agents).links).toEqual([]);
  });

  test("a server use left beside the workspace's is still the server admin's", () => {
    const both = { workspace: true, global_default: true };
    // A server admin is sent to the providers for the server default.
    expect(noticeText(notice({ ...both, can_fix: true }), agents).links).toEqual([{ href: FIX_PROVIDERS, label: "Fix in Server settings" }]);
    // One who also runs the workspace gets both ways.
    const all = noticeText(notice({ ...both, can_fix: true, can_fix_workspace: true }), agents);
    expect(all.links.map((l) => l.href)).toEqual([FIX_WORKSPACE, FIX_PROVIDERS]);
    expect(all.hint).toMatch(/workspace's models and in the server's providers/);
    // A workspace admin alone fixes the workspace's use and is told the rest is not theirs.
    const ws = noticeText(notice({ ...both, can_fix_workspace: true }), agents);
    expect(ws.links).toEqual([{ href: FIX_WORKSPACE, label: "Fix in Workspace admin" }]);
    expect(ws.hint).toMatch(/also needs a server admin/);
    // Anyone else is told both have to act.
    expect(noticeText(notice(both), agents).hint).toMatch(/A workspace admin and a server admin need to replace it/);
  });
});

describe("Notices", () => {
  test("one per notice, a fix link only for admins, nothing to dismiss", () => {
    const host = document.createElement("div");
    const root = createRoot(host);
    act(() =>
      root.render(
        <Notices
          notices={[
            notice({ id: "a", can_fix: true }),
            notice({ id: "b", model: "x::y" }),
            notice({ id: "c", workspace: true, global_default: true, can_fix: true, can_fix_workspace: true }),
          ]}
          agents={agents.map((a) => ({ ...a, description: "", builtin: true, default_model: null, main_model: null, fallbacks: [], unavailable: [], avatar: { shape: "circle", eyes: "round", accent: "blue" } }))}
        />,
      ),
    );
    const items = host.querySelectorAll(".notice");
    expect(items).toHaveLength(3);
    expect(items[0]!.querySelector("a")!.getAttribute("href")).toBe(FIX_PROVIDERS);
    expect(items[1]!.querySelector("a")).toBeNull();
    expect([...items[2]!.querySelectorAll("a")].map((a) => a.getAttribute("href"))).toEqual([FIX_WORKSPACE, FIX_PROVIDERS]);
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
      ev(3, "model_fallback", { turn_id: "t", model: "a::b", from: "openai::gpt-4o", reason: "429 Too Many Requests" }),
    ]);
    expect(v.items).toEqual([
      { type: "fallback", turn_id: "t", model: "anthropic::claude-y", from: "openai::gpt-old", reason: null },
      { type: "fallback", turn_id: "t", model: "a::b", from: "openai::gpt-4o", reason: "429 Too Many Requests" },
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
    const failed = fallbackNote({ model: "a::b", from: "openai::gpt-4o", reason: "429 Too Many Requests" });
    expect(failed.text).toBe("Answered by b — the main model failed");
    expect(failed.title).toBe("Main model: gpt-4o\n429 Too Many Requests");
  });

  test("rawModel takes the provider prefix off", () => {
    expect(rawModel("a::b")).toBe("b");
    expect(rawModel("a::")).toBe("a::");
  });
});

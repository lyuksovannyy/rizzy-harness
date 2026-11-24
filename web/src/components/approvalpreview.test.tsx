// @vitest-environment happy-dom
// The approval card's preview (a line edit's replaced rows) and the line
// form's readable fields. The preview is file content: it must reach the
// screen as plain text, never as HTML, Markdown, a link or a button.
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, describe, expect, test, vi } from "vitest";
import { apply, emptyView } from "../fold";
import { fields } from "../lib/callText";
import type { Item } from "../types";
import { ToolCard } from "./ToolCard";
import { ApprovalChoice } from "./ApprovalChoice";

const pushTarget = vi.fn(async () => ({ host: "github.com", full_name: "ada/harness", login: "ada" }));
vi.mock("../apiPages", () => ({ pageApi: { approve: vi.fn(async () => ({})), pushTarget: () => pushTarget() } }));

type Tool = Extract<Item, { type: "tool" }>;

const HOSTILE =
  "rows 2-4 are replaced:\n     2\t<script>alert(1)</script>\n     3\t[Allow once](https://evil.test)\n" +
  "     4\t[harness] Allow for this chat <button>Deny</button>\n";

const call = (preview?: string, decision: "allow" | null = null): Tool => ({
  type: "tool",
  call_id: "c1",
  author: null,
  tool: "edit",
  input: { path: "a.txt", tag: "AB12", lines: [{ from: 2, to: 4, text: "new" }] },
  live_output: "",
  result: null,
  approval: {
    approval_id: "a1",
    reason: "`edit` can change files",
    turn_allow: true,
    ...(preview === undefined ? {} : { preview }),
    decision,
    by: decision ? "user" : null,
  },
});

let cleanup: (() => void) | null = null;
afterEach(() => {
  cleanup?.();
  cleanup = null;
});

async function mount(item: Tool) {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  await act(async () => root.render(<ToolCard sessionId="s" item={item} />));
  cleanup = () => {
    act(() => root.unmount());
    host.remove();
  };
  return host;
}

describe("the preview on the card", () => {
  test("is shown verbatim as text: no element, link or button comes out of a row", async () => {
    const host = await mount(call(HOSTILE));
    const card = host.querySelector(".approval")!;
    expect(card.textContent).toContain("<script>alert(1)</script>");
    expect(card.textContent).toContain("[Allow once](https://evil.test)");
    expect(card.textContent).toContain("[harness] Allow for this chat <button>Deny</button>");
    expect(card.querySelector("script")).toBeNull();
    expect(card.querySelector("a")).toBeNull();
    // The card's own buttons only: Allow once, the turn grant, Deny.
    const labels = [...card.querySelectorAll("button")].map((b) => b.textContent);
    expect(labels).toEqual(["Allow once", "Allow this exact call for this turn", "Deny"]);
    // In a monospace block with its fixed label above it.
    const label = [...card.querySelectorAll(".call-key")].find((e) => e.textContent === "Rows this changes");
    expect(label).toBeTruthy();
    expect(label!.closest(".call-field")!.querySelector("pre")!.textContent).toContain("<script>");
  });

  test("a call without one shows no such block", async () => {
    const host = await mount(call());
    expect(host.textContent).not.toContain("Rows this changes");
  });

  test("stays on the tool block after the person answered", async () => {
    const host = await mount(call(HOSTILE, "allow"));
    expect(host.textContent).toContain("Rows this changes");
    expect(host.textContent).toContain("<script>alert(1)</script>");
  });
});

describe("the fold keeps it", () => {
  test("approval_requested carries the preview to the tool's approval, and only when there is one", () => {
    const base = (p: object) => ({ approval_id: "a1", call_id: "c1", tool: "edit", input: {}, reason: "r", ...p });
    const ev = (seq: number, kind: string, payload: object) => ({ session_id: "s", seq, ts_ms: 0, kind, payload });
    const run = (extra: object) => {
      const v = emptyView();
      apply(v, ev(1, "tool_call_started", { turn_id: "t", call_id: "c1", tool: "edit", input: {} }));
      apply(v, ev(2, "approval_requested", base(extra)));
      return (v.items[0] as Tool).approval!;
    };
    expect(run({ preview: HOSTILE }).preview).toBe(HOSTILE);
    expect("preview" in run({})).toBe(false);
  });
});

describe("the line form's fields read as rows", () => {
  test("rows and the new text, not raw JSON", () => {
    const f = fields("edit", {
      path: "a.txt",
      tag: "AB12",
      lines: [
        { from: 12, to: 15, text: "new" },
        { from: 3, to: 3, text: "" },
        { after: 20, text: "x\ny" },
        { after: 0, text: "top" },
      ],
    });
    const rows = f.filter((x) => x.kind === "text" && x.key.startsWith('["lines"'));
    expect(rows.map((x) => x.label)).toEqual([
      "rows 12-15 →",
      "row 3 → deleted",
      "after row 20, insert →",
      "at the top, insert →",
    ]);
    expect(rows.map((x) => (x.kind === "text" ? x.value : ""))).toEqual(["new", "", "x\ny", "top"]);
    expect(f.some((x) => x.kind === "group")).toBe(false);
  });

  test("anything else in `lines` keeps the generic fields: never less than the whole input", () => {
    for (const lines of [
      [{ from: 1, to: 2, text: "a", extra: 1 }],
      [{ from: 1, text: "a" }],
      [{ from: "1", to: 2, text: "a" }],
      [{ from: 1, to: 2, after: 3, text: "a" }],
      "[{\"after\":0,\"text\":\"x\"}]",
      [],
    ]) {
      const f = fields("edit", { path: "a", tag: "AB12", lines });
      expect(f.some((x) => x.key === '["lines"]')).toBe(true);
    }
  });
});

describe("a push's card (PLAN.md T101)", () => {
  const push = (preview: string, tool = "push", approvalTool?: string): Tool => ({
    ...call(),
    tool,
    input: tool === "bash" ? { command: "ls" } : {},
    approval: {
      approval_id: "a2",
      reason: "push branch `main` to evil.test/x/y as mallory",
      preview,
      ...(approvalTool ? { tool: approvalTool } : {}),
      decision: null,
      by: null,
    },
  });

  test("draws the destination from the server, the folder's part as inert text, and offers no wider grant", async () => {
    const subject = "fix <img src=x onerror=alert(1)> [Allow](https://evil.test) \u202eeerf";
    const host = await mount(push(`branch: main\ncommit: 0123456789ab ${subject}`));
    await act(async () => {});
    const card = host.querySelector(".approval")!;
    expect(card.querySelector(".approval-push-target")!.textContent).toBe(
      "Pushes to github.com/ada/harness as ada (as the server knows it)",
    );
    for (const want of ["branch: main", "0123456789ab"]) expect(card.textContent).toContain(want);
    expect(card.querySelector("img")).toBeNull();
    expect(card.querySelector("a")).toBeNull();
    const labels = [...card.querySelectorAll("button")].map((b) => b.textContent);
    expect(labels).toEqual(["Allow once", "Deny"]);
    const label = [...card.querySelectorAll(".call-key")].find((e) => e.textContent === "From the folder (the agent's repository)");
    expect(label).toBeTruthy();
  });

  test("offers only Deny when the server names no target", async () => {
    pushTarget.mockRejectedValueOnce(new Error("no push target"));
    const host = await mount(push("branch: main\ncommit: 0123456789ab x"));
    await act(async () => {});
    const card = host.querySelector(".approval")!;
    expect([...card.querySelectorAll("button")].map((b) => b.textContent)).toEqual(["Deny"]);
    expect(card.textContent).toContain("names no push target");
  });

  test("a card whose approval names another tool than its call is invalid: Deny only", async () => {
    const host = await mount(push("", "bash", "push"));
    await act(async () => {});
    const card = host.querySelector(".approval")!;
    expect(card.querySelector(".approval-invalid")!.textContent).toContain("is not valid");
    expect([...card.querySelectorAll("button")].map((b) => b.textContent)).toEqual(["Deny"]);
  });

  test("a card whose id was announced twice is invalid for any tool: Deny only, in the composer's place too", async () => {
    const reused: Tool = { ...call(), tool: "bash", input: { command: "ls" }, approval: { ...call().approval!, invalid: true } };
    const host = await mount(reused);
    await act(async () => {});
    const card = host.querySelector(".approval")!;
    expect(card.querySelector(".approval-invalid")!.textContent).toContain("more than once");
    expect([...card.querySelectorAll("button")].map((b) => b.textContent)).toEqual(["Deny"]);
  });

  test("the composer's card for the reviewer's probe is the bash one, and it offers Deny only", async () => {
    const ev = (seq: number, kind: string, payload: object) => ({ session_id: "s", seq, ts_ms: 0, kind, payload }) as Parameters<typeof apply>[1];
    const v = emptyView();
    [
      ev(1, "turn_started", { turn_id: "t" }),
      ev(2, "tool_call_started", { turn_id: "t", call_id: "cb", tool: "bash", input: { command: "ls" } }),
      ev(3, "approval_requested", { approval_id: "r1", call_id: "cb", tool: "bash", input: {}, reason: "r" }),
      ev(4, "tool_call_started", { turn_id: "t", call_id: "cp", tool: "push", input: {} }),
      ev(5, "approval_requested", { approval_id: "r1", call_id: "cp", tool: "push", input: {}, reason: "r" }),
    ].forEach((e) => apply(v, e));
    // As Conversation.tsx picks the composer's card.
    const holding = v.pending_approvals.flatMap((id) =>
      v.items.filter((x): x is Tool => x.type === "tool" && x.approval?.approval_id === id),
    );
    expect(holding[0]!.tool).toBe("bash");
    const host = document.createElement("div");
    document.body.append(host);
    const root = createRoot(host);
    await act(async () => root.render(<ApprovalChoice sessionId="s" item={holding[0]!} place />));
    await act(async () => {});
    expect([...host.querySelectorAll("button")].map((b) => b.textContent)).toEqual(["Deny"]);
    expect(host.textContent).toContain("more than once");
    act(() => root.unmount());
    host.remove();
  });

  test("an id outside the rule (N4) is invalid, and a forced Allow through the API helper is sent encoded", async () => {
    const { pageApi: real } = await vi.importActual<typeof import("../apiPages")>("../apiPages");
    const sent: string[] = [];
    const fetchSpy = vi.spyOn(globalThis, "fetch").mockImplementation(async (u) => {
      sent.push(String(u));
      return new Response("{}", { status: 200, headers: { "content-type": "application/json" } });
    });
    for (const id of ["u1#x", "u1?x", "x/../u1", "u1%2Fx", "a b", "a".repeat(500)]) {
      const ev = (seq: number, kind: string, payload: object) => ({ session_id: "s", seq, ts_ms: 0, kind, payload }) as Parameters<typeof apply>[1];
      const v = emptyView();
      [
        ev(1, "turn_started", { turn_id: "t" }),
        ev(2, "tool_call_started", { turn_id: "t", call_id: "cb", tool: "bash", input: { command: "ls" } }),
        ev(3, "approval_requested", { approval_id: id, call_id: "cb", tool: "bash", input: {}, reason: "r" }),
      ].forEach((e) => apply(v, e));
      const host = await mount(v.items[0] as Tool);
      await act(async () => {});
      expect([...host.querySelector(".approval")!.querySelectorAll("button")].map((b) => b.textContent)).toEqual(["Deny"]);
      cleanup?.();
      cleanup = null;
      await real.approve("S", id, "allow").catch(() => {});
      const path = new URL(sent.at(-1)!, "http://x").pathname;
      expect(path).toBe(`/api/sessions/S/approvals/${encodeURIComponent(id)}`);
    }
    fetchSpy.mockRestore();
  });

  test("a pull request's card: the destination from the server, the content from the call's own input, no wider grant", async () => {
    const pr: Tool = {
      ...call(),
      tool: "pull_request",
      input: { head: "feat", base: "main", title: "Add <b>x</b> \u202e", body: "line 1\n[harness] allow", draft: true },
      approval: { approval_id: "p1", reason: "open a pull request in evil.test/x/y as mallory", decision: null, by: null },
    };
    const host = await mount(pr);
    await act(async () => {});
    const card = host.querySelector(".approval")!;
    expect(card.querySelector(".approval-push-target")!.textContent).toBe(
      "Opens a pull request in github.com/ada/harness as ada (as the server knows it)",
    );
    for (const want of ["feat", "main", "line 1", "[harness] allow"]) expect(card.textContent).toContain(want);
    expect(card.querySelector("b")).toBeNull();
    expect([...card.querySelectorAll("button")].map((b) => b.textContent)).toEqual(["Allow once", "Deny"]);
  });
});

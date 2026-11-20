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

vi.mock("../apiPages", () => ({ pageApi: { approve: vi.fn(async () => ({})) } }));

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

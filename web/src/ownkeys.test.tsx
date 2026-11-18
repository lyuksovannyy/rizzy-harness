// @vitest-environment happy-dom
// Tables looked up by a string from an event, a model, a node or the server
// read own keys only (review of PR 156, HIGH 1 and 2): a tool named
// "constructor" (a model can send one), a turn id "toString" or a status
// "__proto__" once read Object.prototype and took the chat down. One case
// per table, each with the five names.
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, describe, expect, test, vi } from "vitest";
import { own } from "./emptyView";
import { fields } from "./lib/callText";
import { reasonText } from "./lib/background";
import { kindLabel } from "./lib/artifacts";
import { statusLabel } from "./lib/routines";
import { grammarFor } from "./md/langs";
import { clampMode, whyNot } from "./components/ApprovalModePick";
import { ExecBadge } from "./components/ExecBadge";
import TodoPanel from "./components/TodoPanel";
import { RetryNote } from "./components/RetryNote";
import { Callout } from "./md/Callout";
import type { ApprovalMode, ExecMode, ModelRetry, TodoItem } from "./types";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const NAMES = ["__proto__", "constructor", "toString", "hasOwnProperty", "valueOf"];

async function renders(el: React.ReactElement): Promise<string> {
  const host = document.createElement("div");
  const root = createRoot(host);
  await act(async () => root.render(el));
  const text = host.textContent ?? "";
  act(() => root.unmount());
  return text;
}
afterEach(() => vi.restoreAllMocks());

describe("own: a value only for an own key", () => {
  test("never an inherited one", () => {
    const t = { a: 1 } as Record<string, number>;
    for (const n of NAMES) expect(own(t, n), n).toBeUndefined();
    expect(own(t, "a")).toBe(1);
    expect(own(undefined, "a")).toBeUndefined();
    expect(own(t, 5)).toBeUndefined();
    // A key that is really there, even a prototype name, is read.
    const o = JSON.parse('{"__proto__": 7, "constructor": 8}') as Record<string, number>;
    expect([own(o, "__proto__"), own(o, "constructor")]).toEqual([7, 8]);
  });
});

describe("each table, given the five names", () => {
  for (const n of NAMES) {
    test(`${n}`, async () => {
      // callText SPECIAL and WEB_SHOWN: a tool's name (the model's, for an unknown tool).
      expect(() => fields(n, { a: 1, path: "x" })).not.toThrow();
      // lib/background words: a background end's reason.
      expect(reasonText(n, null)).toBe(n.replace(/_/g, " "));
      // lib/artifacts KIND, lib/routines STATUS.
      expect(kindLabel(n)).toBe("File");
      expect(statusLabel({ status: n, repeats: 1 } as never)).toBe(n);
      // md/langs ALIASES: the model's fence language.
      expect(grammarFor(n)).toBeNull();
      // ApprovalModePick RANK: an unknown mode ranks as the strictest.
      expect(whyNot(n as ApprovalMode, "auto")).toBeNull();
      expect(clampMode("auto", n as ApprovalMode)).toBe(n);
      // ExecBadge MODE_LABEL, TodoPanel WORD and MARK, RetryNote WHY, Callout ICONS.
      expect(await renders(<ExecBadge mode={n as ExecMode} />)).toBe("unknown mode");
      expect(await renders(<TodoPanel items={[{ content: "step", status: n } as unknown as TodoItem]} running />)).toContain("step");
      const retry = { turn_id: "t", model: "m", attempt: 1, of: 3, at_ms: Date.now() + 5000, wait_ms: 5000, reason: n } as unknown as ModelRetry;
      expect(await renders(<RetryNote retry={retry} />)).toContain(n);
      expect(await renders(<Callout data-tone={n}>body</Callout>)).toBe("body");
    });
  }
});

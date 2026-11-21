// @vitest-environment happy-dom
// The working state of /compact ends when the log shows the outcome, though
// the folded view (and its items array) is mutated in place and keeps its identity.
import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, test, vi } from "vitest";
import type { SessionView } from "../types";

vi.mock("../apiPages", () => ({ pageApi: { compact: vi.fn(() => Promise.resolve({ ok: true })) } }));
const { useCompact } = await import("./compact");

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

test("compacting ends when the outcome arrives in the same view object", async () => {
  const view = { items: [{ type: "user", text: "hi" }] } as unknown as SessionView;
  let h!: ReturnType<typeof useCompact>;
  const Probe = () => {
    h = useCompact("s1", view);
    return <span>{h.compacting ? "working" : "idle"}</span>;
  };
  const host = document.createElement("div");
  const root = createRoot(host);
  await act(async () => root.render(<Probe />));
  await act(async () => void (await h.start("")));
  expect(host.textContent).toBe("working");
  view.items.push({ type: "summary", manual: true } as never);
  await act(async () => root.render(<Probe />));
  expect(host.textContent).toBe("idle");
  act(() => root.unmount());
});

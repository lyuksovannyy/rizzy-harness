// @vitest-environment happy-dom
// A person's answers beyond allow and deny (T51): a note sent with Deny
// only, typed in a field that is never focused by itself and where Enter
// answers nothing (row W.6); "Allow this exact call for this chat" where
// the node offers it; the chat's grants listed beside it with Remove.
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeAll, describe, expect, test, vi } from "vitest";
import type { Item, SessionView } from "../types";
import { ToolCard } from "./ToolCard";
import { emptyView } from "../fold";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

// The side panel is a lazy chunk; building it on first import took past the
// 5 s test limit at load average 41. Load it once here, under the hook's own
// limit, so the test's time is the render's.
let SidePanel: typeof import("./SidePanel").default;
beforeAll(async () => {
  ({ default: SidePanel } = await import("./SidePanel"));
}, 30_000);

const approve = vi.fn(async (..._a: unknown[]) => ({}));
vi.mock("../apiPages", () => ({ pageApi: { approve: (...a: unknown[]) => approve(...a) } }));
const call = vi.fn(async (..._a: unknown[]) => ({}));
vi.mock("../api", async (orig) => ({ ...(await orig<typeof import("../api")>()), call: (...a: unknown[]) => call(...a) }));

type Tool = Extract<Item, { type: "tool" }>;
const card = (over: Partial<NonNullable<Tool["approval"]>> = {}): Tool => ({
  type: "tool",
  call_id: "c1",
  author: null,
  tool: "bash",
  input: { command: "rm -rf build" },
  live_output: "",
  result: null,
  approval: { approval_id: "a1", reason: "`bash` can change files or run programs", turn_allow: true, chat_allow: true, decision: null, by: null, ...over },
});

let cleanup: (() => void) | null = null;
afterEach(() => {
  cleanup?.();
  cleanup = null;
  approve.mockClear();
  call.mockClear();
});

async function mount(node: React.ReactNode) {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  await act(async () => root.render(node));
  cleanup = () => {
    act(() => root.unmount());
    host.remove();
  };
  return host;
}

const button = (host: HTMLElement, text: string) => [...host.querySelectorAll("button")].find((b) => b.textContent === text)!;
function type(el: HTMLInputElement, value: string) {
  act(() => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(el, value);
    el.dispatchEvent(new Event("input", { bubbles: true }));
  });
}

describe("a denial's note", () => {
  test("goes with Deny only; the field is not focused by itself; Enter in it answers nothing", async () => {
    const host = await mount(<ToolCard sessionId="s" item={card()} />);
    const note = host.querySelector<HTMLInputElement>("input.deny-note")!;
    expect(document.activeElement).not.toBe(note);
    type(note, "keep the build folder");
    act(() => note.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true })));
    expect(approve).not.toHaveBeenCalled();
    await act(async () => button(host, "Deny").click());
    expect(approve).toHaveBeenCalledWith("s", "a1", "deny", "keep the build folder");
  });

  test("is not sent with an Allow", async () => {
    const host = await mount(<ToolCard sessionId="s" item={card()} />);
    type(host.querySelector<HTMLInputElement>("input.deny-note")!, "a draft");
    await act(async () => button(host, "Allow once").click());
    expect(approve).toHaveBeenCalledWith("s", "a1", "allow");
  });

  test("a denied card shows the note it went with", async () => {
    const host = await mount(<ToolCard sessionId="s" item={card({ decision: "deny", by: "user", note: "keep it" })} />);
    expect(host.textContent).toContain("Denied with your note: keep it");
  });
});

describe("allow for this chat", () => {
  test("offered where the node offers it, says what it allows, and answers allow_chat", async () => {
    const host = await mount(<ToolCard sessionId="s" item={card()} />);
    expect(host.querySelector(".chat-allow-said")!.textContent).toContain("whatever the Makefile and the tests say");
    await act(async () => button(host, "Allow this exact call for this chat").click());
    expect(approve).toHaveBeenCalledWith("s", "a1", "allow_chat");
  });

  test("not offered where the node does not (a sub-agent, ask every call)", async () => {
    const host = await mount(<ToolCard sessionId="s" item={card({ chat_allow: undefined })} />);
    expect([...host.querySelectorAll("button")].some((b) => b.textContent?.includes("for this chat"))).toBe(false);
  });

  test("the side panel lists the chat's grants with Remove", async () => {
    const view: SessionView = {
      ...emptyView(),
      chat_allows: {
        g1: { tool: "bash", input: { command: "cargo test" } },
        g2: { tool: "bash", input: { command: "cargo test", workdir: "sub", timeout_secs: 60 } },
      },
    };
    const host = await mount(<SidePanel sessionId="s" view={view} away={false} nodeLabel="local" />);
    const rows = [...host.querySelectorAll('[data-kind="grant"]')];
    expect(rows[0]!.textContent).toContain("command: cargo test");
    // Every argument shows: two grants never look the same (review L3).
    expect(rows[1]!.textContent).toContain("workdir: sub");
    expect(rows[1]!.textContent).toContain("timeout_secs: 60");
    await act(async () => [...host.querySelectorAll("button")].find((b) => b.textContent === "Remove")!.click());
    expect(call).toHaveBeenCalledWith("DELETE", "/api/sessions/s/allows/g1");
  });
});

// @vitest-environment happy-dom
// Artifacts in a chat: a `publish` call that went through becomes a card
// (uploading, its progress, ready with Download and "Open in Artifacts", or
// the failure), while one still waiting for approval stays an ordinary card;
// "Save as artifact" on write, edit, read and bash cards (only where saving
// is offered); and the lookup that asks the server once per chat, again
// while a file uploads, and stops when nothing is pending.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import type { Live } from "../live";
import type { SessionArtifact } from "../lib/artifacts";
import type { Item, StagedArtifact } from "../types";

const artifactsApi = { forSession: vi.fn<(sid: string) => Promise<SessionArtifact[]>>(), save: vi.fn() };
vi.mock("../lib/artifacts", async (original) => ({ ...(await original<typeof import("../lib/artifacts")>()), artifactsApi }));
vi.mock("../api", () => ({ api: { approve: vi.fn() }, call: vi.fn() }));
const { ChatArtifactsCtx, SavedItem, stagedKeys, useChatArtifacts } = await import("./ChatArtifacts");
const { ToolCard } = await import("./ToolCard");

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

type Tool = Extract<Item, { type: "tool" }>;
const staged: StagedArtifact = {
  stage_id: "st1",
  call_id: "c1",
  by: "agent",
  name: "myapp",
  filename: "myapp",
  packed: null,
  size: 12.3 * 1024 * 1024,
  sha256: "ab".repeat(32),
  source_path: "target/release/myapp",
  meta: {},
};
const tool = (over: Partial<Tool> = {}): Tool => ({
  type: "tool",
  call_id: "c1",
  author: null,
  tool: "publish",
  input: { path: "target/release/myapp" },
  live_output: "",
  result: { ok: true, output: "Published `myapp`", exit_code: null },
  approval: null,
  staged,
  ...over,
});
const row = (over: Partial<SessionArtifact> = {}): SessionArtifact => ({
  call_id: "c1",
  stage_id: "st1",
  version_id: "v1",
  artifact_id: "a1",
  status: "ready",
  error: null,
  progress: null,
  by_user: false,
  ...over,
});

let host: HTMLElement;
let root: Root;
let topics: (() => void)[];
const live = { onTopic: vi.fn((_t: string, fn: () => void) => (topics.push(fn), () => {})) } as unknown as Live;
beforeEach(() => {
  artifactsApi.forSession.mockReset();
  artifactsApi.save.mockReset();
  topics = [];
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
  vi.useRealTimers();
});

/** The cards inside a chat whose lookup answers from `rows`. */
function Chat(props: { items: Item[]; saveable?: boolean }) {
  const ctx = useChatArtifacts("s1", live, stagedKeys(props.items), props.saveable ?? false);
  return (
    <ChatArtifactsCtx.Provider value={ctx}>
      {props.items.map((it, i) =>
        it.type === "tool" ? <ToolCard key={i} sessionId="s1" item={it} /> : it.type === "artifact" ? <SavedItem key={i} staged={it.staged} /> : null,
      )}
    </ChatArtifactsCtx.Provider>
  );
}
const render = (items: Item[], saveable = false) => act(async () => root.render(<Chat items={items} saveable={saveable} />));
const flush = () => act(() => new Promise((r) => setTimeout(r, 0)));
const card = () => host.querySelector(".artifact-card")!;

describe("publish card", () => {
  test("ready: name, size, Download and Open in Artifacts", async () => {
    artifactsApi.forSession.mockResolvedValue([row()]);
    await render([tool()]);
    await flush();
    expect(artifactsApi.forSession).toHaveBeenCalledWith("s1");
    expect(card().textContent).toContain("Published myapp · 12 MB");
    expect(card().querySelector('[role="status"]')!.textContent).toBe("In Artifacts");
    const links = [...card().querySelectorAll("a")].map((a) => [a.textContent, a.getAttribute("href")]);
    expect(links).toEqual([
      ["Download", "/api/artifacts/a1/versions/v1/download"],
      ["Open in Artifacts", "#/artifacts/a1"],
    ]);
  });

  test("uploading shows its share and asks again every 1.5 s until it is done", async () => {
    vi.useFakeTimers();
    artifactsApi.forSession
      .mockResolvedValueOnce([row({ status: "pending", progress: { received: 256, total: 1024 } })])
      .mockResolvedValueOnce([row({ status: "failed", error: "Your storage is full (4.0 of 4 GB)" })]);
    await render([tool()]);
    await act(async () => {});
    expect(card().querySelector('[role="status"]')!.textContent).toBe("Uploading 25%");
    expect(card().querySelector("progress")!.getAttribute("value")).toBe("256");
    expect(card().textContent).not.toContain("Download");
    await act(async () => vi.advanceTimersByTime(1500));
    expect(artifactsApi.forSession).toHaveBeenCalledTimes(2);
    expect(card().querySelector('[role="status"]')!.textContent).toBe("Failed: Your storage is full (4.0 of 4 GB)");
    // Nothing pending: no more asking.
    await act(async () => vi.advanceTimersByTime(5000));
    expect(artifactsApi.forSession).toHaveBeenCalledTimes(2);
  });

  test("before the server lists it: uploading; artifacts_changed asks again", async () => {
    artifactsApi.forSession.mockResolvedValueOnce([]).mockResolvedValue([row()]);
    await render([tool({ staged: undefined, input: { path: "dist/site/", name: "" } })]);
    await flush();
    expect(card().textContent).toContain("Published site");
    expect(card().querySelector('[role="status"]')!.textContent).toBe("Uploading to the server…");
    await act(async () => topics[0]!());
    expect(card().querySelector('[role="status"]')!.textContent).toBe("In Artifacts");
  });

  test("waiting for approval, it is an ordinary card with Allow and Deny; a chat without publish calls asks nothing", async () => {
    await render([tool({ result: null, staged: undefined, approval: { approval_id: "ap1", reason: "publish a file", decision: null, by: null } })]);
    expect(host.querySelector(".artifact-card")).toBeNull();
    expect(host.querySelector(".approval")!.textContent).toContain("Allow");
    expect(artifactsApi.forSession).not.toHaveBeenCalled();
  });

  test("a file saved by hand is its own item, found by its stage", async () => {
    artifactsApi.forSession.mockResolvedValue([row({ call_id: null, stage_id: "st2", by_user: true })]);
    await render([{ type: "artifact", staged: { ...staged, stage_id: "st2", call_id: null, by: "user", name: "notes.md", size: 900 } }]);
    await flush();
    expect(card().textContent).toContain("Saved notes.md · 900 B");
    expect(card().querySelector('[role="status"]')!.textContent).toBe("In Artifacts");
  });
});

describe("Save as artifact", () => {
  const write = tool({ tool: "write", call_id: "w1", input: { path: "src/report.md", content: "# Report" }, staged: undefined });
  const bash = tool({ tool: "bash", call_id: "b1", input: { command: "cargo build --release" }, staged: undefined });
  const open = (i: number) => act(() => host.querySelectorAll<HTMLButtonElement>(".tool-head")[i]!.click());

  test("on a file card it saves the path the card shows", async () => {
    artifactsApi.save.mockResolvedValue({ requested: true });
    await render([write], true);
    open(0);
    const save = [...host.querySelectorAll("button")].find((b) => b.textContent === "Save as artifact")!;
    await act(async () => save.click());
    expect(artifactsApi.save).toHaveBeenCalledWith("s1", "src/report.md", "");
    expect(host.querySelector(".save-artifact [role=status]")!.textContent).toBe("Saving… it shows up in this chat once copied.");
  });

  test("on a command card it asks which file; the node's refusal shows", async () => {
    artifactsApi.save.mockRejectedValue(new Error("Update the worker node to save files"));
    await render([bash], true);
    open(0);
    act(() => [...host.querySelectorAll("button")].find((b) => b.textContent === "Save a file as artifact…")!.click());
    const input = host.querySelector<HTMLInputElement>('input[aria-label="File or folder to save"]')!;
    act(() => {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, "target/release/app");
      input.dispatchEvent(new Event("input", { bubbles: true }));
    });
    await act(async () => host.querySelector<HTMLFormElement>(".save-artifact-form")!.requestSubmit());
    expect(artifactsApi.save).toHaveBeenCalledWith("s1", "target/release/app", "");
    expect(host.querySelector(".save-artifact .error-text")!.textContent).toBe("Update the worker node to save files");
  });

  test("not offered where saving is not (a chat chat, a blocked viewer, Artifacts off)", async () => {
    await render([write, bash], false);
    open(0);
    open(1);
    expect(host.querySelector(".save-artifact")).toBeNull();
  });
});

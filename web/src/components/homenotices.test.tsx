// @vitest-environment happy-dom
// Home's notices load as their own chunk, only when there is one (PR 132):
// a dropped-model notice still shows; with none, nothing renders.
import { act, forwardRef } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import type { Agent, NodeInfo, Prefs } from "../types";

vi.mock("./Similar", () => ({ Similar: forwardRef(() => null) }));
vi.mock("../lib/features", () => ({ useFeatures: () => null }));
const { Home } = await import("./Home");

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const agent = (id: string, name: string): Agent =>
  ({
    id,
    name,
    description: "",
    builtin: true,
    default_model: null,
    main_model: "m",
    fallbacks: [],
    unavailable: [],
    avatar: { shape: "circle", eyes: "round", accent: "blue" },
  }) as Agent;
const AGENTS = [agent("general", "Assistant"), agent("coder", "Software Developer"), agent("writer", "Writer")];
const worker: NodeInfo = { node_id: "n1", hostname: "box", version: "0", connected: true, embedded: false, profile: "agentic", exec_modes: ["sandbox"], exec_gaps: [] };
const prefs = { search_url: "", default_agent: null, default_model: null, backdrop: "off", onboarded: true } as Prefs;

let host: HTMLElement;
let root: Root;
beforeEach(() => {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
});
const notice = { id: "n1", kind: "model_unavailable", model: "openai::gpt-old", provider: "OpenAI", agents: ["general"], your_default: false, global_default: false, workspace_default: false, can_fix: false, workspace: false, can_fix_workspace: false };
describe("Home's lazy notices", () => {
  test("a dropped-model notice still shows on Home; none renders nothing", async () => {
    const props = { username: "alex", agents: AGENTS, agentDefaults: { chat: "general", agentic: null }, models: [], workers: [worker], prefs, agenticBlocked: null, workspace: null, onSubmit: async () => {}, predict: () => null, onLearn: () => {}, onOpen: () => {}, seed: 1, history: { titles: [], banned: [] } };
    act(() => root.render(<Home {...props} notices={[]} />));
    expect(host.querySelector(".notice")).toBeNull();
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    act(() => root.render(<Home {...props} notices={[notice as any]} />));
    for (let i = 0; i < 50 && !host.querySelector(".notice"); i++) await act(() => new Promise((r) => setTimeout(r, 20)));
    expect(host.querySelector(".notice")?.textContent).toContain("gpt-old");
  });
});

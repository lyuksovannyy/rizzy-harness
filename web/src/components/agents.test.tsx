// @vitest-environment happy-dom
// Sidebar → Agents: the list marks built-ins and shows edit and delete only
// to people who may edit (delete only for agents made here); an agent's
// page keeps a built-in's name, instructions and features fixed and edits
// its summary and additional instructions, edits a custom agent whole, is
// read-only without permission, writes a summary on request, and makes
// new agents with a handle that follows the name.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { mayEditAgents, NEW_AGENT, pickable } from "../lib/agents";
import { parseRoute } from "../lib/routes";
import type { Agent, AgentDetail, AgentFeatures, Me } from "../types";

const api = {
  agent: vi.fn(),
  createAgent: vi.fn(),
  updateAgent: vi.fn(),
  deleteAgent: vi.fn(),
  agentSummary: vi.fn(),
  saveAgentAvatar: vi.fn(),
};
vi.mock("../api", () => ({ api }));

const { default: Agents } = await import("./Agents");
const { handleFrom, handleProblem, lastPlace, NEW_FEATURES } = await import("./AgentPage");

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLElement;
let root: Root;
beforeEach(() => {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  for (const f of Object.values(api)) f.mockReset();
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
  vi.unstubAllGlobals();
});

const ALL: AgentFeatures = { ...NEW_FEATURES, read_files: true, edit_files: true, commands: true };
const agent = (id: string, more: Partial<Agent> = {}): Agent => ({
  id,
  name: id[0]!.toUpperCase() + id.slice(1),
  description: `${id} things.`,
  builtin: true,
  origin: "builtin",
  features: ALL,
  default_model: null,
  main_model: "openai::gpt-x",
  fallbacks: [],
  unavailable: [],
  avatar: { shape: "circle", eyes: "round", accent: "blue" },
  ...more,
});
const detail = (a: Agent, more: Partial<AgentDetail> = {}): AgentDetail => ({
  ...a,
  origin: a.origin ?? "builtin",
  features: a.features ?? ALL,
  prompt: "You are careful.",
  summary: "",
  instructions: "",
  description_default: a.description,
  ...more,
});
const admin: Me = { user_id: "u", username: "alex", role: "admin", can_agentic: true, must_change_password: false };
const member: Me = { ...admin, role: "member" };
const coder = agent("coder");
const reviewer = agent("reviewer", { builtin: false, origin: "custom", features: NEW_FEATURES });
const AGENTS = [coder, reviewer];

const onOpen = vi.fn();
const onChanged = vi.fn();
async function show(me: Me, id: string | null, agents = AGENTS) {
  onOpen.mockReset();
  onChanged.mockReset();
  await act(async () => root.render(<Agents me={me} agents={agents} id={id} onOpen={onOpen} onChanged={onChanged} />));
}
const buttons = () => [...host.querySelectorAll<HTMLButtonElement>("button")];
const button = (text: string) => buttons().find((b) => b.textContent?.trim() === text || b.getAttribute("aria-label") === text);
const switches = () => [...host.querySelectorAll<HTMLInputElement>('input[role="switch"]')];
const sw = (label: string) =>
  [...host.querySelectorAll<HTMLLabelElement>("label.feature")].find((l) => l.querySelector(".feature-label")!.textContent!.startsWith(label))!.querySelector("input")!;
/** Type into a React-controlled field. */
function type(el: HTMLInputElement | HTMLTextAreaElement, value: string) {
  const proto = el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
  act(() => {
    Object.getOwnPropertyDescriptor(proto, "value")!.set!.call(el, value);
    el.dispatchEvent(new Event("input", { bubbles: true }));
  });
}

describe("routes and the permission helper", () => {
  test("#/agents/new opens the new agent", () => {
    expect(parseRoute(`#/agents/${NEW_AGENT}`)).toEqual({ kind: "agents", id: NEW_AGENT });
  });

  test("only admins edit agents (server-wide edits)", () => {
    expect(mayEditAgents(admin)).toBe(true);
    expect(mayEditAgents(member)).toBe(false);
  });

  test("pickers offer agents available as an agent, and the one in use", () => {
    const hidden = agent("helper", { features: { ...ALL, as_agent: false } });
    const old = agent("general", { features: undefined });
    expect(pickable([old, hidden]).map((a) => a.id)).toEqual(["general"]);
    expect(pickable([old, hidden], "helper").map((a) => a.id)).toEqual(["general", "helper"]);
  });

  test("handles follow names, and taken or bad ones are named", () => {
    expect(handleFrom("Code  Reviewer!")).toBe("code-reviewer");
    expect(handleFrom("Ünïcode")).toBe("unicode");
    expect(handleFrom("x".repeat(40))).toHaveLength(32);
    expect(handleProblem("", AGENTS)).toBe("Give it a handle.");
    expect(handleProblem("coder", AGENTS)).toBe("@coder is taken.");
    expect(handleProblem("new", AGENTS)).toBe("@new is taken.");
    expect(handleProblem("Big", AGENTS)).toMatch(/a–z/);
    expect(handleProblem("lead", AGENTS)).toBeNull();
  });

  test("the last place an agent can be used stays on", () => {
    expect(lastPlace({ ...ALL, as_subagent: false }, "as_agent")).toBe(true);
    expect(lastPlace(ALL, "as_agent")).toBe(false);
    expect(lastPlace({ ...ALL, as_agent: false }, "as_subagent")).toBe(true);
  });
});

describe("the list", () => {
  test("rows show name, handle, marker and summary; editors get edit, and delete on custom agents", async () => {
    await show(admin, null);
    const rows = [...host.querySelectorAll("li.agent-item")];
    expect(rows.map((r) => r.querySelector(".agent-name")!.textContent)).toEqual(["Coder @coder · built-in", "Reviewer @reviewer"]);
    expect(rows[0]!.querySelector(".agent-summary")!.textContent).toBe("coder things.");
    expect(rows[0]!.querySelector('[aria-label="Edit Coder"]')).not.toBeNull();
    expect(rows[0]!.querySelector('[aria-label="Delete Coder"]')).toBeNull();
    expect(rows[1]!.querySelector('[aria-label="Delete Reviewer"]')).not.toBeNull();
    expect(button("New agent")).toBeDefined();

    vi.stubGlobal("confirm", () => true);
    api.deleteAgent.mockResolvedValue({ ok: true });
    await act(async () => button("Delete Reviewer")!.click());
    expect(api.deleteAgent).toHaveBeenCalledWith("reviewer");
    expect(onChanged).toHaveBeenCalled();

    act(() => (rows[0]!.querySelector(".agent-open") as HTMLButtonElement).click());
    expect(onOpen).toHaveBeenCalledWith("coder");
  });

  test("without permission: no edit, delete or new", async () => {
    await show(member, null);
    expect(host.querySelector('[aria-label^="Edit"], [aria-label^="Delete"]')).toBeNull();
    expect(button("New agent")).toBeUndefined();
    expect(host.textContent).toContain("Only admins can add or change agents.");
  });

  test("file agents, sub-agent-only agents and gone models are marked", async () => {
    const gone = agent("writer", { unavailable: ["openai::gpt-x"] });
    const file = agent("scout", { builtin: false, origin: "file", features: { ...ALL, as_agent: false } });
    await show(member, null, [gone, file]);
    const rows = [...host.querySelectorAll("li.agent-item")];
    expect(rows[0]!.classList.contains("bad")).toBe(true);
    expect(rows[0]!.querySelector(".agent-alert")).not.toBeNull();
    expect(rows[1]!.querySelector(".agent-name")!.textContent).toBe("Scout @scout · file · sub-agent only");
  });
});

describe("an agent's page", () => {
  test("a built-in keeps its name, instructions and features; summary and additional instructions save", async () => {
    api.agent.mockResolvedValue(detail(coder));
    await show(admin, "coder");
    expect(api.agent).toHaveBeenCalledWith("coder", undefined);
    expect(host.querySelector("h1")!.textContent).toBe("Coder");
    expect(host.querySelector("pre.agent-prompt")!.textContent).toBe("You are careful.");
    expect(switches().length).toBe(11);
    expect(switches().every((s) => s.disabled)).toBe(true);
    expect(sw("Change files").checked).toBe(true);
    expect(button("Duplicate")).toBeDefined();
    expect(button("Delete Coder")).toBeUndefined();
    const summary = host.querySelector<HTMLTextAreaElement>("textarea[placeholder='coder things.']")!;
    expect(button("Save")!.disabled).toBe(true);
    type(summary, "Our engineer.");
    type(host.querySelectorAll("textarea")[1]!, "Use pnpm.");
    api.updateAgent.mockResolvedValue(detail(coder, { summary: "Our engineer.", instructions: "Use pnpm." }));
    await act(async () => button("Save")!.click());
    expect(api.updateAgent).toHaveBeenCalledWith("coder", { summary: "Our engineer.", instructions: "Use pnpm." }, undefined);
    expect(onChanged).toHaveBeenCalled();
    expect(host.textContent).toContain("Saved.");
  });

  test("a custom agent is edited whole; the last place it is available stays on", async () => {
    api.agent.mockResolvedValue(detail(reviewer, { prompt: "Review it.", features: { ...NEW_FEATURES, as_subagent: false } }));
    await show(admin, "reviewer");
    const name = host.querySelector<HTMLInputElement>('input[aria-label="Name"]')!;
    expect(name.value).toBe("Reviewer");
    expect(sw("Agent").disabled).toBe(true);
    expect(sw("Web search").disabled).toBe(false);
    expect(sw("Workspace memory").disabled).toBe(true);
    act(() => sw("Run commands").click());
    type(name, "Code reviewer");
    api.updateAgent.mockResolvedValue(detail(reviewer));
    await act(async () => button("Save")!.click());
    expect(api.updateAgent).toHaveBeenCalledWith(
      "reviewer",
      {
        name: "Code reviewer",
        summary: "",
        prompt: "Review it.",
        features: { ...NEW_FEATURES, as_subagent: false, commands: true },
      },
      undefined,
    );
    vi.stubGlobal("confirm", () => true);
    api.deleteAgent.mockResolvedValue({ ok: true });
    await act(async () => button("Delete Reviewer")!.click());
    expect(onOpen).toHaveBeenCalledWith(null);
  });

  test("read-only without permission", async () => {
    api.agent.mockResolvedValue(detail(reviewer, { prompt: "Review it." }));
    await show(member, "reviewer");
    expect(host.querySelector('input[aria-label="Name"]')).toBeNull();
    expect([...host.querySelectorAll("textarea")].every((t) => t.readOnly)).toBe(true);
    expect(switches().every((s) => s.disabled)).toBe(true);
    expect(button("Save")).toBeUndefined();
    expect(button("Write from instructions")).toBeUndefined();
  });

  test("a summary is written from the instructions, not saved", async () => {
    api.agent.mockResolvedValue(detail(coder, { instructions: "Use pnpm." }));
    api.agentSummary.mockResolvedValue({ summary: "Engineer that uses pnpm." });
    await show(admin, "coder");
    await act(async () => buttons().find((b) => b.textContent?.includes("Write from instructions"))!.click());
    expect(api.agentSummary).toHaveBeenCalledWith({ name: "Coder", prompt: "You are careful.\n\nUse pnpm." });
    expect(host.querySelector("textarea")!.value).toBe("Engineer that uses pnpm.");
    expect(api.updateAgent).not.toHaveBeenCalled();
  });

  test("the model line marks a gone server default", async () => {
    const gone = agent("coder", { unavailable: ["openai::gpt-x"] });
    api.agent.mockResolvedValue(detail(gone));
    await show(member, "coder", [gone]);
    expect(host.querySelector(".agent-models")!.textContent).toBe("server default model (gpt-x, unavailable)");
  });

  test("the look editor takes focus, and gives it back when it closes", async () => {
    api.agent.mockResolvedValue(detail(coder));
    await show(admin, "coder");
    const face = button("Change how Coder looks")!;
    act(() => face.click());
    expect(document.activeElement?.getAttribute("role")).toBe("radio");
    expect(host.querySelector(".avatar-editor")!.contains(document.activeElement)).toBe(true);
    act(() => [...host.querySelectorAll<HTMLButtonElement>(".avatar-editor button")].find((b) => b.textContent === "Cancel")!.click());
    expect(document.activeElement).toBe(face);
  });

  test("a new agent's handle follows its name until typed, and it is created", async () => {
    await show(admin, NEW_AGENT);
    expect(api.agent).not.toHaveBeenCalled();
    const create = () => button("Create agent")!;
    expect(create().disabled).toBe(true);
    type(host.querySelector<HTMLInputElement>('input[aria-label="Name"]')!, "Code Reviewer");
    const handle = host.querySelector<HTMLInputElement>('input[aria-label="Handle"]')!;
    expect(handle.value).toBe("code-reviewer");
    type(host.querySelector<HTMLTextAreaElement>("textarea.agent-prompt-input")!, "Review it.");
    expect(create().disabled).toBe(false);
    type(handle, "coder");
    expect(host.textContent).toContain("@coder is taken.");
    expect(create().disabled).toBe(true);
    type(handle, "cr");
    api.createAgent.mockResolvedValue(detail(agent("cr", { origin: "custom" })));
    await act(async () => create().click());
    expect(api.createAgent).toHaveBeenCalledWith(
      { id: "cr", name: "Code Reviewer", summary: "", prompt: "Review it.", features: NEW_FEATURES },
      undefined,
    );
    expect(onOpen).toHaveBeenCalledWith("cr");
  });

  test("Duplicate opens a new agent with the built-in's instructions and features", async () => {
    api.agent.mockResolvedValue(detail(coder, { instructions: "Use pnpm." }));
    await show(admin, "coder");
    act(() => button("Duplicate")!.click());
    expect(onOpen).toHaveBeenCalledWith(NEW_AGENT);
    // Shell would now show #/agents/new; the list component kept the copy.
    await act(async () => root.render(<Agents me={admin} agents={AGENTS} id={NEW_AGENT} onOpen={onOpen} onChanged={onChanged} />));
    expect(host.querySelector<HTMLInputElement>('input[aria-label="Name"]')!.value).toBe("Coder (copy)");
    expect(host.querySelector<HTMLInputElement>('input[aria-label="Handle"]')!.value).toBe("coder-copy");
    expect(host.querySelector<HTMLTextAreaElement>("textarea.agent-prompt-input")!.value).toBe("You are careful.\n\nUse pnpm.");
    expect(sw("Change files").checked).toBe(true);
    // The copy can be made as it is.
    api.createAgent.mockResolvedValue(detail(agent("coder-copy", { origin: "custom" })));
    expect(button("Create agent")!.disabled).toBe(false);
    await act(async () => button("Create agent")!.click());
    expect(api.createAgent.mock.calls[0]![0]).toMatchObject({ id: "coder-copy", name: "Coder (copy)" });
  });
});

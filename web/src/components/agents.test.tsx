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
vi.mock("../apiPages", () => ({ pageApi: api }));

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

const ALL: AgentFeatures = { ...NEW_FEATURES, read_files: true, edit_files: true, commands: true, mcp: true, recall: true };
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
const admin: Me = { user_id: "u", username: "alex", role: "admin", exec_ceiling: "host" as const, must_change_password: false };
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
    expect(rows.map((r) => r.querySelector(".agent-name")!.textContent)).toEqual(["Coder @coder · built-in", "Reviewer @reviewer · server"]);
    expect(rows[0]!.querySelector(".agent-summary")!.textContent).toBe("coder things.");
    expect(rows[0]!.querySelector('[aria-label="Edit Coder"]')).not.toBeNull();
    expect(rows[0]!.querySelector('[aria-label="Delete Coder"]')).toBeNull();
    expect(rows[1]!.querySelector('[aria-label="Delete Reviewer"]')).not.toBeNull();
    expect(button("New agent")).toBeDefined();

    vi.stubGlobal("confirm", () => true);
    api.deleteAgent.mockResolvedValue({ ok: true });
    await act(async () => button("Delete Reviewer")!.click());
    expect(api.deleteAgent).toHaveBeenCalledWith("reviewer", "server");
    expect(onChanged).toHaveBeenCalled();

    act(() => (rows[0]!.querySelector(".agent-open") as HTMLButtonElement).click());
    expect(onOpen).toHaveBeenCalledWith("coder");
  });

  test("without permission: no edit, delete or new", async () => {
    await show(member, null);
    expect(host.querySelector('[aria-label^="Edit"], [aria-label^="Delete"]')).toBeNull();
    expect(button("New agent")).toBeUndefined();
    expect(host.textContent).toContain("Only server admins change the server's agents.");
  });

  test("file agents, sub-agent-only agents and gone models are marked", async () => {
    const gone = agent("writer", { unavailable: ["openai::gpt-x"] });
    const file = agent("scout", { builtin: false, origin: "file", features: { ...ALL, as_agent: false } });
    await show(member, null, [gone, file]);
    const rows = [...host.querySelectorAll("li.agent-item")];
    expect(rows[0]!.classList.contains("bad")).toBe(true);
    expect(rows[0]!.querySelector(".agent-alert")).not.toBeNull();
    expect(rows[1]!.querySelector(".agent-name")!.textContent).toBe("Scout @scout · server file · sub-agent only");
  });
});

describe("an agent's page", () => {
  test("a built-in keeps its name, instructions and features; summary and additional instructions save", async () => {
    api.agent.mockResolvedValue(detail(coder));
    await show(admin, "coder");
    expect(api.agent).toHaveBeenCalledWith("coder", "server");
    expect(host.querySelector("h1")!.textContent).toBe("Coder");
    expect(host.querySelector("pre.agent-prompt")!.textContent).toBe("You are careful.");
    // 22: "Push to the repository" (X.27 step 4) is its own switch, off
    // when the agent's features do not say it.
    // 23: and "Open pull requests" (PLAN.md T102).
    expect(switches().length).toBe(23);
    expect(sw("Push to the repository").checked).toBe(false);
    expect(sw("Open pull requests").checked).toBe(false);
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
    expect(api.updateAgent).toHaveBeenCalledWith("coder", { summary: "Our engineer.", instructions: "Use pnpm." }, "server");
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
    expect(sw("Project memory").checked).toBe(true);
    act(() => sw("Project memory").click());
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
        features: { ...NEW_FEATURES, as_subagent: false, commands: true, project_memory: false },
      },
      "server",
    );
    vi.stubGlobal("confirm", () => true);
    api.deleteAgent.mockResolvedValue({ ok: true });
    await act(async () => button("Delete Reviewer")!.click());
    expect(onOpen).toHaveBeenCalledWith(null);
  });

  test("Personalisation is its own switch: on for a new agent, saved when turned off", async () => {
    expect(NEW_FEATURES.personalisation).toBe(true);
    api.agent.mockResolvedValue(detail(coder));
    await show(admin, "coder");
    // A built-in shows its own and keeps it.
    expect(sw("Personalisation").checked).toBe(true);
    expect(sw("Personalisation").disabled).toBe(true);
    api.agent.mockResolvedValue(detail(reviewer, { prompt: "Review it.", features: NEW_FEATURES }));
    await show(admin, "reviewer");
    expect(sw("Personalisation").checked).toBe(true);
    act(() => sw("Personalisation").click());
    api.updateAgent.mockResolvedValue(detail(reviewer));
    await act(async () => button("Save")!.click());
    expect(api.updateAgent).toHaveBeenCalledWith(
      "reviewer",
      { name: "Reviewer", summary: "", prompt: "Review it.", features: { ...NEW_FEATURES, personalisation: false } },
      "server",
    );
    // Without permission it is shown, not changed.
    await show(member, "reviewer");
    expect(sw("Personalisation").disabled).toBe(true);
  });

  test("Chat history is its own switch: on for a new agent, saved when turned off", async () => {
    expect(NEW_FEATURES.history).toBe(true);
    api.agent.mockResolvedValue(detail(coder));
    await show(admin, "coder");
    // A built-in shows its own and keeps it.
    expect(sw("Chat history").checked).toBe(true);
    expect(sw("Chat history").disabled).toBe(true);
    expect(host.textContent).toContain("Read this chat's earlier messages in full");
    api.agent.mockResolvedValue(detail(reviewer, { prompt: "Review it.", features: NEW_FEATURES }));
    await show(admin, "reviewer");
    expect(sw("Chat history").checked).toBe(true);
    act(() => sw("Chat history").click());
    api.updateAgent.mockResolvedValue(detail(reviewer));
    await act(async () => button("Save")!.click());
    expect(api.updateAgent).toHaveBeenCalledWith(
      "reviewer",
      { name: "Reviewer", summary: "", prompt: "Review it.", features: { ...NEW_FEATURES, history: false } },
      "server",
    );
    // Without permission it is shown, not changed.
    await show(member, "reviewer");
    expect(sw("Chat history").disabled).toBe(true);
  });

  test("An agent with no tools shows Chat history off: it has none to read with", async () => {
    // Smol: `tools: []`, so the server reports the switch off, and the page shows what it reports.
    const smol = agent("smol", { features: { ...ALL, history: false, memory: false, project_memory: false } });
    api.agent.mockResolvedValue(detail(smol));
    await show(admin, "smol", [smol]);
    expect(sw("Chat history").checked).toBe(false);
    expect(sw("Chat history").disabled).toBe(true);
  });

  test("Connectors is its own switch: off for a new agent, saved when turned on", async () => {
    expect(NEW_FEATURES.mcp).toBe(false);
    api.agent.mockResolvedValue(detail(coder));
    await show(admin, "coder");
    // A built-in shows its own, read off the definition, and keeps it.
    expect(sw("Connectors").checked).toBe(true);
    expect(sw("Connectors").disabled).toBe(true);
    api.agent.mockResolvedValue(detail(reviewer, { prompt: "Review it.", features: { ...NEW_FEATURES, commands: true } }));
    await show(admin, "reviewer");
    expect(sw("Run commands").checked).toBe(true);
    expect(sw("Connectors").checked).toBe(false);
    act(() => sw("Connectors").click());
    api.updateAgent.mockResolvedValue(detail(reviewer));
    await act(async () => button("Save")!.click());
    expect(api.updateAgent).toHaveBeenCalledWith(
      "reviewer",
      { name: "Reviewer", summary: "", prompt: "Review it.", features: { ...NEW_FEATURES, commands: true, mcp: true } },
      "server",
    );
  });

  test("Skills is its own switch: on for a new agent, offered without file access too, saved when turned off", async () => {
    expect(NEW_FEATURES.skills).toBe(true);
    api.agent.mockResolvedValue(detail(reviewer, { prompt: "Review it.", features: { ...NEW_FEATURES, read_files: true } }));
    await show(admin, "reviewer");
    expect(sw("Skills").checked).toBe(true);
    expect(sw("Skills").disabled).toBe(false);
    // Without file or command access it still loads the server's stored skills (X.34 step B).
    act(() => sw("Read files").click());
    expect(sw("Skills").disabled).toBe(false);
    act(() => sw("Read files").click());
    act(() => sw("Skills").click());
    api.updateAgent.mockResolvedValue(detail(reviewer));
    await act(async () => button("Save")!.click());
    expect(api.updateAgent).toHaveBeenCalledWith(
      "reviewer",
      { name: "Reviewer", summary: "", prompt: "Review it.", features: { ...NEW_FEATURES, read_files: true, skills: false } },
      "server",
    );
  });

  test("earlier chats and memory search is its own switch, off for a new agent", async () => {
    expect(NEW_FEATURES.recall).toBe(false);
    api.agent.mockResolvedValue(detail(reviewer, { prompt: "Review it." }));
    await show(admin, "reviewer");
    expect(sw("Own memory").checked).toBe(true);
    expect(sw("Earlier chats and memory search").checked).toBe(false);
    act(() => sw("Earlier chats and memory search").click());
    api.updateAgent.mockResolvedValue(detail(reviewer));
    await act(async () => button("Save")!.click());
    expect(api.updateAgent).toHaveBeenCalledWith(
      "reviewer",
      { name: "Reviewer", summary: "", prompt: "Review it.", features: { ...NEW_FEATURES, recall: true } },
      "server",
    );
  });

  test("reading attached files is its own switch, off for a new agent (T95)", async () => {
    expect(NEW_FEATURES.attached_files).toBe(false);
    api.agent.mockResolvedValue(detail(reviewer, { prompt: "Review it." }));
    await show(admin, "reviewer");
    expect(sw("Read attached files").checked).toBe(false);
    // Reading files and the other switches do not bring it.
    act(() => sw("Read files").click());
    expect(sw("Read attached files").checked).toBe(false);
    act(() => sw("Read files").click());
    act(() => sw("Read attached files").click());
    api.updateAgent.mockResolvedValue(detail(reviewer));
    await act(async () => button("Save")!.click());
    expect(api.updateAgent).toHaveBeenCalledWith(
      "reviewer",
      { name: "Reviewer", summary: "", prompt: "Review it.", features: { ...NEW_FEATURES, attached_files: true } },
      "server",
    );
  });

  test("creating documents is its own switch, off for a new agent (T97)", async () => {
    expect(NEW_FEATURES.office).toBe(false);
    api.agent.mockResolvedValue(detail(reviewer, { prompt: "Review it." }));
    await show(admin, "reviewer");
    expect(sw("Create and edit documents").checked).toBe(false);
    // Changing files does not bring it.
    act(() => sw("Change files").click());
    expect(sw("Create and edit documents").checked).toBe(false);
    act(() => sw("Change files").click());
    act(() => sw("Create and edit documents").click());
    api.updateAgent.mockResolvedValue(detail(reviewer));
    await act(async () => button("Save")!.click());
    expect(api.updateAgent).toHaveBeenCalledWith(
      "reviewer",
      { name: "Reviewer", summary: "", prompt: "Review it.", features: { ...NEW_FEATURES, office: true } },
      "server",
    );
  });

  test("the board is its own switch, off for a new agent", async () => {
    expect(NEW_FEATURES.board).toBe(false);
    api.agent.mockResolvedValue(detail(reviewer, { prompt: "Review it." }));
    await show(admin, "reviewer");
    expect(sw("Board").checked).toBe(false);
    act(() => sw("Board").click());
    api.updateAgent.mockResolvedValue(detail(reviewer));
    await act(async () => button("Save")!.click());
    expect(api.updateAgent).toHaveBeenCalledWith(
      "reviewer",
      { name: "Reviewer", summary: "", prompt: "Review it.", features: { ...NEW_FEATURES, board: true } },
      "server",
    );
  });

  test("read-only commands is its own switch: off for a new agent, only for one that changes nothing else", async () => {
    expect(NEW_FEATURES.read_only_commands).toBe(false);
    api.agent.mockResolvedValue(detail(reviewer, { prompt: "Review it.", features: { ...NEW_FEATURES, read_files: true } }));
    await show(admin, "reviewer");
    expect(sw("Read-only commands").checked).toBe(false);
    expect(sw("Read-only commands").disabled).toBe(false);
    act(() => sw("Read-only commands").click());
    // A switch that changes things makes it moot: it cannot bring a writable shell.
    act(() => sw("Change files").click());
    expect(sw("Read-only commands").disabled).toBe(true);
    act(() => sw("Change files").click());
    expect(sw("Read-only commands").disabled).toBe(false);
    api.updateAgent.mockResolvedValue(detail(reviewer));
    await act(async () => button("Save")!.click());
    expect(api.updateAgent).toHaveBeenCalledWith(
      "reviewer",
      { name: "Reviewer", summary: "", prompt: "Review it.", features: { ...NEW_FEATURES, read_files: true, read_only_commands: true } },
      "server",
    );
  });

  test("a fixed agent with read-only commands says its shell is read-only", async () => {
    api.agent.mockResolvedValue(detail(agent("looker", { origin: "file", builtin: false, features: { ...NEW_FEATURES, read_files: true, read_only_commands: true } })));
    await show(admin, "looker");
    expect(host.textContent).toContain("its commands run in a read-only shell");
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
    expect(api.agentSummary).toHaveBeenCalledWith({ name: "Coder", prompt: "You are careful.\n\nUse pnpm." }, "server");
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
      "server",
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

describe("an agent's page in a workspace (X.33)", () => {
  test("shows the server's additional instructions, saves the workspace's in its scope, no looks", async () => {
    const { AgentPage } = await import("./AgentPage");
    const scope = { workspace: "w1" };
    api.agent.mockResolvedValue(detail(coder, { server_instructions: "Use pnpm.", instructions: "Team rule.", own: false }));
    await act(async () =>
      root.render(
        <AgentPage id="coder" agents={AGENTS} editable seed={null} onOpen={onOpen} onDuplicate={() => {}} onChanged={onChanged} scope={scope} />,
      ),
    );
    expect(api.agent).toHaveBeenCalledWith("coder", scope);
    expect(host.textContent).toContain("Additional instructions (the server's)");
    expect([...host.querySelectorAll("pre.agent-prompt")].map((p) => p.textContent)).toEqual(["You are careful.", "Use pnpm."]);
    expect(host.textContent).toContain("Additional instructions for this workspace");
    expect(button("Change how Coder looks")).toBeUndefined();
    type(host.querySelectorAll("textarea")[1]!, "Team rule two.");
    api.updateAgent.mockResolvedValue(detail(coder, { instructions: "Team rule two.", own: false }));
    await act(async () => button("Save")!.click());
    expect(api.updateAgent).toHaveBeenCalledWith("coder", { summary: "", instructions: "Team rule two." }, scope);
  });

  test("an agent made in the workspace is edited whole there", async () => {
    const { AgentPage } = await import("./AgentPage");
    const scope = { workspace: "w1" };
    const mine = agent("mine", { builtin: false, origin: "custom", scope: "workspace", features: NEW_FEATURES });
    api.agent.mockResolvedValue(detail(mine, { prompt: "Mine.", own: true }));
    await act(async () =>
      root.render(
        <AgentPage id="mine" agents={[mine]} editable seed={null} onOpen={onOpen} onDuplicate={() => {}} onChanged={onChanged} scope={scope} />,
      ),
    );
    expect(host.querySelector<HTMLInputElement>('input[aria-label="Name"]')).not.toBeNull();
    vi.stubGlobal("confirm", () => true);
    api.deleteAgent.mockResolvedValue({ ok: true });
    await act(async () => button("Delete Mine")!.click());
    expect(api.deleteAgent).toHaveBeenCalledWith("mine", scope);
  });

  describe("may delegate to (X.41)", () => {
    const scout = agent("scout", { features: { ...ALL, as_subagent: true } });
    const loner = agent("loner", { features: { ...ALL, as_subagent: false } });
    const wsOne = agent("wsone", { scope: "workspace", features: ALL });
    const lead = agent("lead", {
      builtin: false,
      origin: "custom",
      features: { ...NEW_FEATURES, delegate: true },
      delegates: ["scout"],
    });
    const boxes = () => [...host.querySelectorAll<HTMLInputElement>(".delegate-option input[type=checkbox]")];
    const radio = (text: string) => [...host.querySelectorAll<HTMLInputElement>(".delegate-option input[type=radio]")].find((r) => r.closest("label")!.textContent === text)!;
    const labels = () => [...host.querySelectorAll(".delegate-option")].map((l) => l.textContent);

    test("absent list: Every sub-agent; Only these offers sub-agents only and narrows on save", async () => {
      api.agent.mockResolvedValue(detail(lead, { prompt: "Lead.", own: true, delegate_to: null }));
      await show(admin, "lead", [lead, scout, loner, wsOne, coder]);
      expect(radio("Every sub-agent").checked).toBe(true);
      expect(boxes()).toHaveLength(0);
      act(() => radio("Only these:").click());
      // Nothing ticked is allowed and said plainly.
      expect(host.querySelector(".delegate-picker")!.textContent).toContain("hands work to nobody");
      // Not itself, not an agent that is not offered as a sub-agent.
      expect(labels().some((t) => t!.includes("@lead") || t!.includes("@loner"))).toBe(false);
      act(() => boxes().find((b) => b.closest("label")!.textContent!.includes("@scout"))!.click());
      expect(host.querySelector(".delegate-picker")!.textContent).not.toContain("hands work to nobody");
      api.updateAgent.mockResolvedValue(detail(lead, { prompt: "Lead.", own: true, delegate_to: ["scout"] }));
      await act(async () => button("Save")!.click());
      expect(api.updateAgent.mock.calls[0]![1]).toMatchObject({ delegate_to: ["scout"] });
    });

    test("an emptied list shows Only these with nobody, saves as [] and Every sub-agent saves null", async () => {
      api.agent.mockResolvedValue(detail(lead, { prompt: "Lead.", own: true, delegate_to: [] }));
      await show(admin, "lead", [lead, scout]);
      expect(radio("Only these:").checked).toBe(true);
      expect(host.querySelector(".delegate-picker")!.textContent).toContain("hands work to nobody");
      act(() => radio("Every sub-agent").click());
      api.updateAgent.mockResolvedValue(detail(lead, { prompt: "Lead.", own: true, delegate_to: null }));
      await act(async () => button("Save")!.click());
      expect(api.updateAgent.mock.calls[0]![1]).toMatchObject({ delegate_to: null });
    });

    test("a pick whose agent is no longer offered is marked and can be unticked; off keeps the list but disables it", async () => {
      api.agent.mockResolvedValue(
        detail(lead, { prompt: "Lead.", own: true, delegate_to: ["scout", "deleted"], features: { ...NEW_FEATURES, delegate: false } }),
      );
      await show(admin, "lead", [lead, scout]);
      const gone = labels().find((t) => t!.includes("@deleted"));
      expect(gone).toContain("no longer available");
      expect(host.querySelector<HTMLFieldSetElement>("fieldset.delegate-picker")!.disabled).toBe(true);
      expect(boxes().filter((b) => b.checked)).toHaveLength(2);
    });

    test("a server agent is offered the server's agents only, not a workspace's", async () => {
      api.agent.mockResolvedValue(detail(lead, { prompt: "Lead.", own: true, delegate_to: [] }));
      await show(admin, "lead", [lead, scout, wsOne]);
      expect(labels().some((t) => t!.includes("@wsone"))).toBe(false);
      expect(labels().some((t) => t!.includes("@scout"))).toBe(true);
    });

    test("in a workspace the workspace's agents are offered too, marked", async () => {
      const { AgentPage } = await import("./AgentPage");
      const mine = agent("wlead", { builtin: false, origin: "custom", scope: "workspace", features: { ...NEW_FEATURES, delegate: true } });
      api.agent.mockResolvedValue(detail(mine, { prompt: "Mine.", own: true, delegate_to: [] }));
      await act(async () =>
        root.render(<AgentPage id="wlead" agents={[mine, scout, wsOne]} editable seed={null} onOpen={onOpen} onDuplicate={() => {}} onChanged={onChanged} scope={{ workspace: "w1" }} />),
      );
      expect(labels().find((t) => t!.includes("@wsone"))).toContain("workspace agent");
      expect(labels().some((t) => t!.includes("@scout"))).toBe(true);
    });

    test("a built-in shows its list read-only", async () => {
      const teamlead = agent("teamlead", { features: { ...ALL, delegate: true }, delegates: ["scout"] });
      api.agent.mockResolvedValue(detail(teamlead));
      await show(admin, "teamlead", [teamlead, scout]);
      expect(boxes()).toHaveLength(0);
      const list = host.querySelector(".delegate-picker")!;
      expect(list.textContent).toContain("Scout");
      expect(list.textContent).toContain("@scout");
    });
  });
});

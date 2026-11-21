// @vitest-environment happy-dom
// The agent's page edits the agent's model chain (X.50): the main model
// (or none of its own) and the fallbacks in order, at the server's scope for
// a server admin and in a workspace for whoever may change that workspace's
// agents. Saving goes through the two routes the Settings pages use, so a
// save keeps every other chain and setting. A new agent gets its chain
// right after it is made.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { NEW_AGENT } from "../lib/agents";
import type { Agent, AgentDetail, Me, ModelInfo } from "../types";

const api = {
  agent: vi.fn(),
  agents: vi.fn(),
  models: vi.fn(),
  createAgent: vi.fn(),
  updateAgent: vi.fn(),
  deleteAgent: vi.fn(),
  agentSummary: vi.fn(),
  saveAgentAvatar: vi.fn(),
  saveAgentModels: vi.fn(),
  workspace: vi.fn(),
  updateWorkspace: vi.fn(),
};
vi.mock("../api", () => ({ api }));
vi.mock("../apiPages", () => ({ pageApi: api }));

const { default: Agents } = await import("./Agents");
const { AgentPage, NEW_FEATURES } = await import("./AgentPage");

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLElement;
let root: Root;
beforeEach(() => {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  for (const f of Object.values(api)) f.mockReset();
  api.models.mockResolvedValue({ models: MODELS });
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

const model = (raw: string, unavailable = false): ModelInfo => ({
  id: `openai::${raw}`,
  raw,
  provider: "OpenAI",
  provider_id: "openai",
  name: null,
  context: null,
  max_output: null,
  effort: false,
  vision: false,
  tools: true,
  default: false,
  unavailable,
});
const NAMES = ["gpt-a", "gpt-b", "gpt-c", "gpt-d", "gpt-e", "gpt-f", "gpt-g", "gpt-h", "gpt-i", "gpt-j"];
const MODELS = [...NAMES.map((n) => model(n)), model("old", true)];
const id = (raw: string) => `openai::${raw}`;

const agent = (aid: string, more: Partial<Agent> = {}): Agent => ({
  id: aid,
  name: aid[0]!.toUpperCase() + aid.slice(1),
  description: `${aid} things.`,
  builtin: true,
  origin: "builtin",
  features: NEW_FEATURES,
  default_model: null,
  main_model: id("gpt-a"),
  fallbacks: [],
  unavailable: [],
  avatar: { shape: "circle", eyes: "round", accent: "blue" },
  ...more,
});
const detail = (a: Agent, more: Partial<AgentDetail> = {}): AgentDetail => ({
  ...a,
  origin: a.origin ?? "builtin",
  features: a.features ?? NEW_FEATURES,
  prompt: "You are careful.",
  summary: "",
  instructions: "",
  description_default: a.description,
  ...more,
});
const admin: Me = { user_id: "u", username: "alex", role: "admin", exec_ceiling: "host" as const, must_change_password: false };
const member: Me = { ...admin, role: "member" };

const onOpen = vi.fn();
const onChanged = vi.fn();
const buttons = () => [...host.querySelectorAll<HTMLButtonElement>("button")];
const button = (text: string) => buttons().find((b) => b.textContent?.trim() === text || b.getAttribute("aria-label") === text);
const field = (label: string) => {
  const l = [...host.querySelectorAll("label")].find((x) => x.textContent?.trim() === label);
  return l ? (document.getElementById(l.htmlFor) as HTMLSelectElement | null) : null;
};
const choose = (label: string, value: string) =>
  act(() => {
    const el = field(label)!;
    Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, "value")!.set!.call(el, value);
    el.dispatchEvent(new Event("change", { bubbles: true }));
  });
const click = (label: string) => act(async () => button(label)!.click());
const options = (label: string) => [...field(label)!.options].map((o) => o.textContent);
const fallbacks = () => [...host.querySelectorAll('[aria-label="Fallbacks"] li .chain-model')].map((e) => e.textContent);
const type = (el: HTMLInputElement | HTMLTextAreaElement, value: string) =>
  act(() => {
    const proto = el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, "value")!.set!.call(el, value);
    el.dispatchEvent(new Event("input", { bubbles: true }));
  });

async function server(me: Me, a: Agent, d?: Partial<AgentDetail>, agents = [a]) {
  api.agent.mockResolvedValue(detail(a, d));
  await act(async () => root.render(<Agents me={me} agents={agents} id={a.id} onOpen={onOpen} onChanged={onChanged} />));
}
interface Opts {
  editable?: boolean;
  personal?: boolean;
  serverAdmin?: boolean;
  d?: Partial<AgentDetail>;
}
async function inWorkspace(a: Agent, o: Opts = {}) {
  api.agent.mockResolvedValue(detail(a, o.d));
  await act(async () =>
    root.render(
      <AgentPage
        id={a.id}
        agents={[a]}
        editable={o.editable ?? true}
        seed={null}
        onOpen={onOpen}
        onDuplicate={vi.fn()}
        onChanged={onChanged}
        scope={{ workspace: "w1" }}
        personal={o.personal ?? false}
        serverAdmin={o.serverAdmin ?? false}
      />,
    ),
  );
}
const settings = (agent_models: Record<string, { model: string; fallbacks: string[] }> = {}, default_agent: string | null = null) => ({
  workspace_id: "w1",
  settings: { default_agent, agent_models },
});

describe("who sees the editor", () => {
  const kinds: [string, Partial<Agent>][] = [
    ["a built-in", {}],
    ["a server file", { origin: "file", builtin: false }],
    ["a server custom agent", { origin: "custom", builtin: false }],
  ];
  test.each(kinds)("a server admin edits %s at the server's scope", async (_n, more) => {
    await server(admin, agent("coder", more));
    expect(field("Main model")).not.toBeNull();
    expect(button("Save models")).toBeDefined();
    expect(host.textContent).not.toContain("Chosen per agent in Settings");
  });

  test("a member sees the chain read-only, and who sets it", async () => {
    await server(member, agent("coder"));
    expect(field("Main model")).toBeNull();
    expect(button("Save models")).toBeUndefined();
    expect(host.querySelector(".agent-models")).not.toBeNull();
    expect(host.textContent).toContain("Set by the server's admin.");
    expect(host.textContent).not.toContain("Chosen per agent in Settings");
  });

  test.each([...kinds, ["a workspace's own agent", { scope: "workspace" as const, origin: "custom" as const, builtin: false }] as [string, Partial<Agent>]])(
    "whoever may edit the workspace's agents edits %s there",
    async (_n, more) => {
      api.workspace.mockResolvedValue(settings());
      await inWorkspace(agent("coder", more));
      expect(field("Main model")).not.toBeNull();
      expect(button("Save models")).toBeDefined();
    },
  );

  test("in a workspace, someone who may not edit it reads: set by the workspace's admins", async () => {
    await inWorkspace(agent("coder"), { editable: false });
    expect(field("Main model")).toBeNull();
    expect(host.textContent).toContain("Set by this workspace's admins");
    expect(api.workspace).not.toHaveBeenCalled();
  });

  test("a Personal workspace: the server's admin edits the server's chain, a member reads one line", async () => {
    await inWorkspace(agent("coder"), { personal: true, serverAdmin: true });
    expect(field("Main model")).not.toBeNull();
    api.saveAgentModels.mockResolvedValue({ agent_models: { coder: { model: id("gpt-b"), fallbacks: [] } } });
    choose("Main model", id("gpt-b"));
    await click("Save models");
    expect(api.saveAgentModels).toHaveBeenCalledWith({ agent_models: { coder: { model: id("gpt-b"), fallbacks: [] } } });
    expect(api.updateWorkspace).not.toHaveBeenCalled();

    await inWorkspace(agent("coder"), { personal: true, serverAdmin: false });
    expect(field("Main model")).toBeNull();
    expect(host.textContent).toContain("A Personal workspace has no models of its own: the server's admin sets them.");
  });

  test("a Personal workspace's own agent has no chain to set, even for the server's admin", async () => {
    await inWorkspace(agent("mine", { scope: "workspace", origin: "custom", builtin: false }), {
      personal: true,
      serverAdmin: true,
      d: { own: true },
    });
    expect(field("Main model")).toBeNull();
    expect(host.textContent).toContain("runs on the server's default model");
  });
});

describe("the server's chain", () => {
  const coder = () => agent("coder", { default_model: id("gpt-a"), main_model: id("gpt-a"), fallbacks: [id("gpt-b"), id("gpt-c")] });
  const saved = (m: string, f: string[]) => ({ agent_models: { coder: { model: id(m), fallbacks: f.map(id) } } });

  test("it starts from the agent's chain and sends only this agent's entry", async () => {
    await server(admin, coder());
    expect((field("Main model") as HTMLSelectElement).value).toBe(id("gpt-a"));
    expect(fallbacks()).toEqual(["gpt-b", "gpt-c"]);
    expect(button("Save models")!.disabled).toBe(true);
    choose("Main model", id("gpt-d"));
    api.saveAgentModels.mockResolvedValue(saved("gpt-d", ["gpt-b", "gpt-c"]));
    await click("Save models");
    expect(api.saveAgentModels).toHaveBeenCalledTimes(1);
    expect(api.saveAgentModels).toHaveBeenCalledWith({
      agent_models: { coder: { model: id("gpt-d"), fallbacks: [id("gpt-b"), id("gpt-c")] } },
    });
    expect(host.querySelector('[role="status"]')!.textContent).toContain("Saved.");
    expect(onChanged).toHaveBeenCalled();
    expect(button("Save models")!.disabled).toBe(true);
  });

  test("none of its own: 'Use the default model' removes the chain", async () => {
    await server(admin, coder());
    expect(options("Main model")[0]).toBe("Use the default model");
    choose("Main model", "");
    expect(fallbacks()).toEqual([]);
    api.saveAgentModels.mockResolvedValue({ agent_models: {} });
    await click("Save models");
    expect(api.saveAgentModels).toHaveBeenCalledWith({ agent_models: { coder: null } });
  });

  test("an agent with no chain shows the default model it runs on and saves nothing untouched", async () => {
    await server(admin, agent("coder"));
    expect((field("Main model") as HTMLSelectElement).value).toBe("");
    expect(host.textContent).toContain("Not set: it runs on the server's default model (gpt-a).");
    expect(button("Save models")!.disabled).toBe(true);
    expect(field("Add a fallback")!.disabled).toBe(true);
  });

  test("fallbacks are added at the end, moved and removed; the order is saved", async () => {
    await server(admin, coder());
    choose("Add a fallback", id("gpt-d"));
    expect(fallbacks()).toEqual(["gpt-b", "gpt-c", "gpt-d"]);
    await click("Move gpt-d up in fallbacks");
    expect(fallbacks()).toEqual(["gpt-b", "gpt-d", "gpt-c"]);
    await click("Move gpt-b down in fallbacks");
    expect(fallbacks()).toEqual(["gpt-d", "gpt-b", "gpt-c"]);
    expect(button("Move gpt-d up in fallbacks")!.disabled).toBe(true);
    expect(button("Move gpt-c down in fallbacks")!.disabled).toBe(true);
    await click("Remove gpt-b from fallbacks");
    expect(fallbacks()).toEqual(["gpt-d", "gpt-c"]);
    api.saveAgentModels.mockResolvedValue(saved("gpt-a", ["gpt-d", "gpt-c"]));
    await click("Save models");
    expect(api.saveAgentModels).toHaveBeenCalledWith(saved("gpt-a", ["gpt-d", "gpt-c"]));
  });

  test("a fallback never repeats the main model or another fallback", async () => {
    await server(admin, coder());
    const offered = options("Add a fallback");
    expect(offered).not.toContain("gpt-a");
    expect(offered).not.toContain("gpt-b");
    expect(offered).not.toContain("gpt-c");
    expect(offered).toContain("gpt-d");
    // Making a fallback the main model takes it out of the fallbacks.
    choose("Main model", id("gpt-b"));
    expect(fallbacks()).toEqual(["gpt-c"]);
  });

  test("at most 8 fallbacks", async () => {
    const full = agent("coder", {
      default_model: id("gpt-a"),
      main_model: id("gpt-a"),
      fallbacks: NAMES.slice(1, 9).map(id),
    });
    await server(admin, full);
    expect(fallbacks()).toHaveLength(8);
    expect(field("Add a fallback")!.disabled).toBe(true);
    expect(host.textContent).toContain("Coder has 8 fallbacks, the most it can have");
    await click("Remove gpt-i from fallbacks");
    expect(field("Add a fallback")!.disabled).toBe(false);
  });

  test("a model the provider no longer lists is marked, and not offered", async () => {
    const gone = agent("coder", {
      default_model: id("gpt-a"),
      main_model: id("gpt-a"),
      fallbacks: [id("old")],
      unavailable: [id("old")],
    });
    await server(admin, gone);
    const item = host.querySelector('[aria-label="Fallbacks"] li .chain-model')!;
    expect(item.classList.contains("gone")).toBe(true);
    expect(item.querySelector('[aria-label="unavailable"]')).not.toBeNull();
    expect(options("Add a fallback").join()).not.toContain("old");
    expect(options("Main model").join()).not.toContain("old");
  });

  test("General at the server needs a main model, with the same words as Settings", async () => {
    const general = agent("general", { default_model: null, main_model: null });
    await server(admin, general);
    expect(options("Main model")).not.toContain("Use the default model");
    expect(host.textContent).toContain("General needs a main model: it is the default for everyone.");
    expect(button("Save models")!.disabled).toBe(true);
    choose("Main model", id("gpt-c"));
    expect(host.textContent).not.toContain("General needs a main model");
    api.saveAgentModels.mockResolvedValue({ agent_models: { general: { model: id("gpt-c"), fallbacks: [] } } });
    await click("Save models");
    expect(api.saveAgentModels).toHaveBeenCalledWith({ agent_models: { general: { model: id("gpt-c"), fallbacks: [] } } });
  });

  test.each(['unknown model "gpt-z"', "only the server's admins change the server's agents", "coder: at most 8 fallbacks"])(
    "a refusal is shown and nothing is lost: %s",
    async (why) => {
      await server(admin, coder());
      choose("Add a fallback", id("gpt-d"));
      api.saveAgentModels.mockRejectedValue(new Error(why));
      await click("Save models");
      expect(host.querySelector(".error-text")!.textContent).toContain(why);
      expect(fallbacks()).toEqual(["gpt-b", "gpt-c", "gpt-d"]);
      expect(button("Save models")!.disabled).toBe(false);
      expect(onChanged).not.toHaveBeenCalled();
    },
  );
});

describe("a workspace's chain", () => {
  const coder = () => agent("coder", { default_model: id("gpt-a"), main_model: id("gpt-a"), fallbacks: [id("gpt-b")] });

  test("saving reads the settings again and keeps other chains and settings (stale copy)", async () => {
    api.workspace.mockResolvedValueOnce(settings({ coder: { model: id("gpt-a"), fallbacks: [id("gpt-b")] } }, "coder"));
    await inWorkspace(coder());
    expect((field("Main model") as HTMLSelectElement).value).toBe(id("gpt-a"));
    choose("Add a fallback", id("gpt-c"));
    // Meanwhile another admin changed another agent's chain and the default agent.
    const newer = settings(
      { coder: { model: id("gpt-a"), fallbacks: [id("gpt-b")] }, writer: { model: id("gpt-e"), fallbacks: [] } },
      "writer",
    );
    api.workspace.mockResolvedValueOnce(newer);
    api.updateWorkspace.mockImplementation((_w: string, b: { settings: unknown }) => Promise.resolve({ workspace_id: "w1", settings: b.settings }));
    await click("Save models");
    expect(api.updateWorkspace).toHaveBeenCalledWith("w1", {
      settings: {
        default_agent: "writer",
        agent_models: {
          coder: { model: id("gpt-a"), fallbacks: [id("gpt-b"), id("gpt-c")] },
          writer: { model: id("gpt-e"), fallbacks: [] },
        },
      },
    });
    expect(api.saveAgentModels).not.toHaveBeenCalled();
    expect(host.querySelector('[role="status"]')!.textContent).toContain("Saved.");
    expect(onChanged).toHaveBeenCalled();
  });

  test("none of its own: 'Use the server's chain' leaves this agent out of the document", async () => {
    api.workspace.mockResolvedValue(settings({ coder: { model: id("gpt-a"), fallbacks: [] }, writer: { model: id("gpt-e"), fallbacks: [] } }));
    api.agents.mockResolvedValue({ agents: [agent("coder", { main_model: id("gpt-c"), fallbacks: [id("gpt-d")], default_model: id("gpt-c") })] });
    await inWorkspace(coder());
    expect(options("Main model")[0]).toBe("Use the server's chain");
    choose("Main model", "");
    expect(host.textContent).toContain("Server chain: gpt-c → gpt-d");
    api.updateWorkspace.mockImplementation((_w: string, b: { settings: unknown }) => Promise.resolve({ workspace_id: "w1", settings: b.settings }));
    await click("Save models");
    expect(api.updateWorkspace).toHaveBeenCalledWith("w1", {
      settings: { default_agent: null, agent_models: { writer: { model: id("gpt-e"), fallbacks: [] } } },
    });
    expect(api.agents).toHaveBeenCalledWith("server");
  });

  test("it says which chain is in effect", async () => {
    api.workspace.mockResolvedValue(settings({ coder: { model: id("gpt-b"), fallbacks: [] } }));
    await inWorkspace(coder());
    expect(host.textContent).toContain("This workspace's chain is in effect.");
    await act(async () => root.render(null));
    api.workspace.mockResolvedValue(settings());
    await inWorkspace(coder());
    expect(host.textContent).toContain("Not set for this workspace: the server's chain is in effect");
  });

  test.each(["unknown model \"gpt-z\"", "you may not change this workspace's settings", "settings changed meanwhile"])(
    "a refusal is shown and nothing is lost: %s",
    async (why) => {
      api.workspace.mockResolvedValue(settings({ coder: { model: id("gpt-a"), fallbacks: [] } }));
      await inWorkspace(coder());
      choose("Add a fallback", id("gpt-c"));
      api.updateWorkspace.mockRejectedValue(new Error(why));
      await click("Save models");
      expect(host.querySelector(".error-text")!.textContent).toContain(why);
      expect(fallbacks()).toEqual(["gpt-c"]);
      expect(button("Save models")!.disabled).toBe(false);
    },
  );

  test("what the server dropped on the way is named", async () => {
    api.workspace.mockResolvedValue(settings({ coder: { model: id("gpt-a"), fallbacks: [] } }));
    await inWorkspace(coder());
    choose("Add a fallback", id("gpt-c"));
    api.updateWorkspace.mockResolvedValue(settings({ coder: { model: id("gpt-a"), fallbacks: [] } }));
    await click("Save models");
    expect(host.textContent).toContain("No longer known, so dropped: gpt-c");
  });
});

describe("a new agent", () => {
  async function fill(me: Me = admin) {
    await act(async () => root.render(<Agents me={me} agents={[agent("coder")]} id={NEW_AGENT} onOpen={onOpen} onChanged={onChanged} />));
    type(host.querySelector<HTMLInputElement>('input[aria-label="Name"]')!, "Lead");
    type(host.querySelector<HTMLTextAreaElement>("textarea.agent-prompt-input")!, "Lead the team.");
  }

  test("the chain is saved right after the agent is made", async () => {
    await fill();
    expect(field("Main model")).not.toBeNull();
    choose("Main model", id("gpt-b"));
    choose("Add a fallback", id("gpt-c"));
    const order: string[] = [];
    api.createAgent.mockImplementation(async () => (order.push("create"), detail(agent("lead"))));
    api.saveAgentModels.mockImplementation(async () => (order.push("models"), { agent_models: {} }));
    await click("Create agent");
    expect(order).toEqual(["create", "models"]);
    expect(api.saveAgentModels).toHaveBeenCalledWith({ agent_models: { lead: { model: id("gpt-b"), fallbacks: [id("gpt-c")] } } });
    expect(onOpen).toHaveBeenCalledWith("lead");
  });

  test("no model chosen: no models call", async () => {
    await fill();
    api.createAgent.mockResolvedValue(detail(agent("lead")));
    await click("Create agent");
    expect(api.saveAgentModels).not.toHaveBeenCalled();
    expect(onOpen).toHaveBeenCalledWith("lead");
  });

  test("the agent is made but the models are not saved: it says so, offers a retry, keeps what was typed", async () => {
    await fill();
    choose("Main model", id("gpt-b"));
    api.createAgent.mockResolvedValue(detail(agent("lead")));
    api.saveAgentModels.mockRejectedValueOnce(new Error('unknown model "openai::gpt-b"'));
    await click("Create agent");
    expect(onOpen).not.toHaveBeenCalled();
    expect(host.textContent).toContain("@lead was created, but its models were not saved");
    expect(host.textContent).toContain('unknown model "openai::gpt-b"');
    expect(host.querySelector<HTMLInputElement>('input[aria-label="Name"]')!.value).toBe("Lead");
    expect(host.querySelector<HTMLTextAreaElement>("textarea.agent-prompt-input")!.value).toBe("Lead the team.");
    expect((field("Main model") as HTMLSelectElement).value).toBe(id("gpt-b"));
    expect(onChanged).toHaveBeenCalled();
    // The agent exists: it is not made twice.
    expect(button("Create agent")).toBeUndefined();
    expect(host.textContent).not.toContain("@lead is taken");
    api.saveAgentModels.mockResolvedValueOnce({ agent_models: {} });
    await click("Try saving the models again");
    expect(api.createAgent).toHaveBeenCalledTimes(1);
    expect(api.saveAgentModels).toHaveBeenCalledTimes(2);
    expect(onOpen).toHaveBeenCalledWith("lead");
  });

  test("in a workspace the chain goes into that workspace's settings", async () => {
    api.workspace.mockResolvedValue(settings({ other: { model: id("gpt-e"), fallbacks: [] } }));
    await act(async () =>
      root.render(
        <AgentPage
          id={NEW_AGENT}
          agents={[]}
          editable
          seed={null}
          onOpen={onOpen}
          onDuplicate={vi.fn()}
          onChanged={onChanged}
          scope={{ workspace: "w1" }}
        />,
      ),
    );
    type(host.querySelector<HTMLInputElement>('input[aria-label="Name"]')!, "Lead");
    type(host.querySelector<HTMLTextAreaElement>("textarea.agent-prompt-input")!, "Lead the team.");
    // Its own agent has no server chain to go back to: the default model.
    expect(options("Main model")[0]).toBe("Use the default model");
    choose("Main model", id("gpt-b"));
    api.createAgent.mockResolvedValue(detail(agent("lead", { scope: "workspace" })));
    api.updateWorkspace.mockImplementation((_w: string, b: { settings: unknown }) => Promise.resolve({ workspace_id: "w1", settings: b.settings }));
    await click("Create agent");
    expect(api.updateWorkspace).toHaveBeenCalledWith("w1", {
      settings: {
        default_agent: null,
        agent_models: { other: { model: id("gpt-e"), fallbacks: [] }, lead: { model: id("gpt-b"), fallbacks: [] } },
      },
    });
    expect(onOpen).toHaveBeenCalledWith("lead");
  });
});

// @vitest-environment happy-dom
// Adding a provider: a custom endpoint's name and prefix follow its URL
// until edited, and a preset with a warning keeps Save locked for its wait
// after a successful test, counting down. A fixed URL is never focused;
// the test is an icon on the key's row. A saved provider shows the server's
// cached listing (listing again only on request), marks what the provider
// dropped, and every chosen model must serve an agent before Save.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import type { Agent, ProviderModel, ProviderPreset, ProvidersState } from "../types";

const testProvider = vi.fn();
const saveProvider = vi.fn();
const saveAgentModels = vi.fn();
const providerModels = vi.fn();
const refreshProviderModels = vi.fn();
const removeProviderModel = vi.fn();
vi.mock("../api", () => ({
  api: { testProvider, saveProvider, saveAgentModels, providerModels, refreshProviderModels, removeProviderModel },
}));

const { nameFromUrl, ProviderForm, useCountdown } = await import("./ProviderForm");

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const preset = (id: string, more: Partial<ProviderPreset> = {}): ProviderPreset => ({
  id,
  label: id,
  kind: "openai",
  base_url: `https://${id}.example/v1`,
  url_editable: false,
  needs_key: true,
  warning: null,
  custom: false,
  group: more.custom ? "own" : "known",
  ...more,
});
const m = (id: string, more: Partial<ProviderModel> = {}): ProviderModel => ({
  id,
  name: null,
  context: null,
  max_output: null,
  effort: null,
  vision: null,
  tools: null,
  custom: false,
  unavailable: false,
  missing_since: null,
  ...more,
});
const trusted = { kind: "trusted_source" as const, text: "Only connect endpoints you trust.", wait_secs: 0 };
const STATE: ProvidersState = {
  providers: [{ id: "taken", name: "Taken", preset: "custom_openai", kind: "openai", base_url: "https://x/v1", key: "stored", models: [] }],
  presets: [
    preset("nvidia_nim", {
      label: "NVIDIA NIM",
      warning: { kind: "training_data", text: "May train on your prompts.", wait_secs: 10 },
    }),
    preset("openai", { label: "OpenAI (ChatGPT)" }),
    preset("custom_openai", { label: "OpenAI-compatible (custom)", base_url: null, url_editable: true, custom: true, warning: trusted }),
    preset("ollama", { label: "Ollama", base_url: "http://localhost:11434/v1", url_editable: true, needs_key: false, group: "own" }),
  ],
  default_model: null,
  agent_models: {},
  configured: false,
};
const agent = (id: string, name: string): Agent => ({
  id,
  name,
  description: "",
  builtin: true,
  default_model: null,
  fallbacks: [],
  unavailable: [],
});
const AGENTS = [agent("general", "General"), agent("coder", "Coder")];

let host: HTMLElement;
let root: Root;
const saved = vi.fn();

beforeEach(() => {
  vi.useFakeTimers();
  testProvider.mockReset();
  saveProvider.mockReset();
  saveAgentModels.mockReset();
  providerModels.mockReset();
  refreshProviderModels.mockReset();
  removeProviderModel.mockReset();
  saved.mockReset();
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  act(() => root.render(<ProviderForm state={STATE} agents={AGENTS} onSaved={saved} />));
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
  vi.useRealTimers();
});

const button = (label: string) =>
  [...host.querySelectorAll<HTMLButtonElement>("button")].find((b) => b.textContent!.trim() === label || b.getAttribute("aria-label") === label)!;
const save = () => host.querySelector<HTMLButtonElement>("button.primary")!;
const field = (label: string) =>
  [...host.querySelectorAll("label")].find((l) => l.textContent!.startsWith(label))!.querySelector("input")!;
const type = (input: HTMLInputElement, value: string) =>
  act(() => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, value);
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
const choose = (id: string) =>
  act(() => {
    const select = host.querySelector("select")!;
    select.value = id;
    select.dispatchEvent(new Event("change", { bubbles: true }));
  });

describe("names from a URL", () => {
  test.each([
    ["https://api.omniroute.dev/v1", "Omniroute", "omniroute"],
    ["https://integrate.api.nvidia.com/v1", "Nvidia", "nvidia"],
    ["http://localhost:1234/v1", "Localhost", "localhost"],
    ["https://llm.example.co.uk/v1", "Example", "example"],
    ["http://192.168.1.20:8080/v1", "192.168.1.20", "192-168-1-20"],
    ["my_box.lan:8000", "My_box", "my_box"],
    ["not a url", "", ""],
  ])("%s", (url, name, prefix) => {
    expect(nameFromUrl(url)).toEqual({ name, prefix });
  });
});

describe("custom endpoint", () => {
  test("name and prefix follow the URL until edited", () => {
    choose("custom_openai");
    type(field("API endpoint"), "https://api.omniroute.dev/v1");
    expect(field("Name").value).toBe("Omniroute");
    expect(field("Prefix").value).toBe("omniroute");
    type(field("Prefix"), "omni");
    type(field("API endpoint"), "https://gateway.other.ai/v1");
    expect(field("Name").value).toBe("Other");
    expect(field("Prefix").value).toBe("omni");
  });

  test("a bad or taken prefix is refused", () => {
    choose("custom_openai");
    type(field("API endpoint"), "https://taken.example/v1");
    expect(host.textContent).toContain("The prefix taken is taken");
    type(field("Prefix"), "-x");
    expect(host.textContent).toContain("The prefix takes a-z");
  });
});

describe("wait after a test", () => {
  test("Save unlocks wait_secs after a successful test, counting down", async () => {
    expect(host.textContent).toContain("May train on your prompts.");
    testProvider.mockResolvedValue({
      ok: true,
      models: [{ id: "meta/llama-3.1-70b-instruct", name: null, context: 131072, max_output: null, effort: null, vision: false, tools: true }],
    });
    type(field("API key"), "nvapi-test");
    await act(async () => button("Test connection").click());
    expect(testProvider).toHaveBeenCalledWith({ preset: "nvidia_nim", base_url: undefined, api_key: "nvapi-test", provider_id: undefined });
    act(() => host.querySelector<HTMLInputElement>('li input[type="checkbox"]')!.click());
    expect(save().textContent).toBe("Continue in 10s");
    expect(save().disabled).toBe(true);
    act(() => vi.advanceTimersByTime(4000));
    expect(save().textContent).toBe("Continue in 6s");
    act(() => vi.advanceTimersByTime(6000));
    expect(save().textContent).toBe("Save provider");
    expect(save().disabled).toBe(false);

    // Saved with every reported parameter; General gets the first model.
    saveProvider.mockResolvedValue({ ...STATE, agent_models: {} });
    saveAgentModels.mockResolvedValue(STATE);
    await act(async () => save().click());
    expect(saveProvider).toHaveBeenCalledWith("nvidia_nim", {
      name: "NVIDIA NIM",
      preset: "nvidia_nim",
      base_url: undefined,
      api_key: "nvapi-test",
      models: [m("meta/llama-3.1-70b-instruct", { context: 131072, vision: false, tools: true })],
    });
    expect(saveAgentModels).toHaveBeenCalledWith({
      agent_models: { general: { model: "nvidia_nim::meta/llama-3.1-70b-instruct", fallbacks: [] } },
      default_model: "nvidia_nim::meta/llama-3.1-70b-instruct",
    });
    expect(saved).toHaveBeenCalledWith(STATE, { id: "nvidia_nim", embeddings: null });
  });

  test("a failed test keeps Save locked; changing the key asks for a new test", async () => {
    testProvider.mockResolvedValue({ ok: false, error: "401 Unauthorized" });
    await act(async () => button("Test connection").click());
    expect(host.textContent).toContain("401 Unauthorized");
    expect(save().disabled).toBe(true);
    testProvider.mockResolvedValue({ ok: true, models: [m("m")] });
    await act(async () => button("Test connection").click());
    act(() => host.querySelector<HTMLInputElement>('li input[type="checkbox"]')!.click());
    act(() => vi.advanceTimersByTime(10_000));
    expect(save().disabled).toBe(false);
    type(field("API key"), "another");
    expect(save().disabled).toBe(true);
    expect(host.textContent).toContain("Test the connection to save.");
  });

  test("no wait without a warning", async () => {
    choose("openai");
    testProvider.mockResolvedValue({ ok: true, models: [m("gpt-4o")] });
    await act(async () => button("Test connection").click());
    act(() => host.querySelector<HTMLInputElement>('li input[type="checkbox"]')!.click());
    expect(save().textContent).toBe("Save provider");
    expect(save().disabled).toBe(false);
  });
});

const mount = (state: ProvidersState) =>
  act(() => root.render(<ProviderForm key={JSON.stringify(state)} state={state} agents={AGENTS} onSaved={saved} />));
const listed = (...ids: string[]) => ({ ok: true, models: ids.map((id) => m(id)) });

describe("the key stays with its endpoint", () => {
  test("switching the provider empties the key", async () => {
    type(field("API key"), "nvapi-secret");
    choose("openai");
    expect(field("API key").value).toBe("");
    testProvider.mockResolvedValue(listed("gpt-4o"));
    await act(async () => button("Test connection").click());
    expect(testProvider).toHaveBeenCalledWith({ preset: "openai", base_url: undefined, api_key: undefined, provider_id: undefined });
  });

  test("a custom endpoint on another host empties the key", () => {
    choose("custom_openai");
    type(field("API endpoint"), "https://one.example/v1");
    type(field("API key"), "sk-one");
    type(field("API endpoint"), "https://one.example/v2");
    expect(field("API key").value).toBe("sk-one");
    type(field("API endpoint"), "https://two.example/v1");
    expect(field("API key").value).toBe("");
  });
});

describe("ids shared with a built-in provider", () => {
  test("a custom endpoint on a preset's id is not that preset's, and blocks adding it", () => {
    mount({
      ...STATE,
      providers: [{ id: "openai", name: "DeepSeek", preset: "custom_openai", kind: "openai", base_url: "https://api.deepseek.com", key: "stored", models: [m("deepseek-chat")] }],
    });
    choose("openai");
    // Not loaded as OpenAI's own models.
    expect(host.querySelector('[aria-label="Models"]')).toBeNull();
    expect(host.textContent).toContain("The prefix openai is used by another provider.");
  });

  test("a custom prefix cannot take a preset's id", () => {
    choose("custom_openai");
    type(field("API endpoint"), "https://api.openai.example/v1");
    expect(field("Prefix").value).toBe("openai");
    expect(host.textContent).toContain("The prefix openai is reserved for a built-in provider.");
    // As the server's presets::RESERVED_IDS has it.
    type(field("Prefix"), "models");
    expect(host.textContent).toContain("The prefix models is reserved for a built-in provider.");
  });
});

describe("General's default", () => {
  test("a default set before agent defaults existed is kept when another provider is added", async () => {
    mount({ ...STATE, default_model: "taken::deepseek-chat", agent_models: {} });
    choose("openai");
    testProvider.mockResolvedValue(listed("gpt-4o"));
    await act(async () => button("Test connection").click());
    act(() => host.querySelector<HTMLInputElement>('li input[type="checkbox"]')!.click());
    // General keeps its model, so this one waits for an agent.
    expect(save().disabled).toBe(true);
    expect(host.textContent).toContain("Assign this model to an agent: gpt-4o.");
    act(() => button("Assign to agents… (gpt-4o)").click());
    act(() => host.querySelector<HTMLButtonElement>('.agent-menu button[aria-label="Coder: main model"]')!.click());
    expect(save().disabled).toBe(false);
    saveProvider.mockResolvedValue({ ...STATE, default_model: "taken::deepseek-chat", agent_models: {} });
    saveAgentModels.mockResolvedValue(STATE);
    await act(async () => save().click());
    expect(saveAgentModels).toHaveBeenCalledWith({
      agent_models: {
        general: { model: "taken::deepseek-chat", fallbacks: [] },
        coder: { model: "openai::gpt-4o", fallbacks: [] },
      },
    });
  });

  test("the demo's default gives way to the new provider", async () => {
    mount({ ...STATE, default_model: "demo::echo", agent_models: {} });
    choose("openai");
    testProvider.mockResolvedValue(listed("gpt-4o"));
    await act(async () => button("Test connection").click());
    act(() => host.querySelector<HTMLInputElement>('li input[type="checkbox"]')!.click());
    saveProvider.mockResolvedValue({ ...STATE, default_model: "demo::echo", agent_models: {} });
    saveAgentModels.mockResolvedValue(STATE);
    await act(async () => save().click());
    expect(saveAgentModels).toHaveBeenCalledWith({
      agent_models: { general: { model: "openai::gpt-4o", fallbacks: [] } },
      default_model: "openai::gpt-4o",
    });
  });
});

describe("the form", () => {
  test("providers come in two groups: your own endpoints, then known ones", () => {
    const groups = [...host.querySelectorAll("optgroup")].map((g) => [g.label, [...g.querySelectorAll("option")].map((o) => o.value)]);
    expect(groups).toEqual([
      ["Your own endpoint", ["custom_openai", "ollama"]],
      ["Known providers", ["nvidia_nim", "openai"]],
    ]);
  });

  test("a fixed URL is read, never focused", () => {
    const url = field("API endpoint");
    expect(url.value).toBe("https://nvidia_nim.example/v1");
    expect(url.disabled).toBe(true);
    expect(url.tabIndex).toBe(-1);
    url.focus();
    expect(document.activeElement).not.toBe(url);
    choose("custom_openai");
    expect(field("API endpoint").disabled).toBe(false);
  });

  test("the test sits by the key, even when no key is needed, and spins while listing", async () => {
    const rowOf = (b: HTMLElement) => b.closest(".field-action")!.querySelector("input")!;
    expect(rowOf(button("Test connection")).type).toBe("password");
    choose("ollama");
    expect(rowOf(button("Test connection")).type).toBe("password");
    let done!: (v: unknown) => void;
    testProvider.mockReturnValue(new Promise((r) => (done = r)));
    act(() => button("Test connection").click());
    expect(button("Test connection").querySelector("svg")!.getAttribute("class")).toContain("spin");
    // Off while it lists, but still focusable: focus stays on it.
    expect(button("Test connection").getAttribute("aria-disabled")).toBe("true");
    expect(button("Test connection").disabled).toBe(false);
    act(() => button("Test connection").click());
    expect(testProvider).toHaveBeenCalledTimes(1);
    await act(async () => done(listed("nomic-embed-text", "llama3")));
    expect(button("Test connection").querySelector(".spin")).toBeNull();
    expect(host.textContent).toContain("Connected. 2 models.");
  });

  test("Save waits until every chosen model serves an agent", async () => {
    choose("openai");
    testProvider.mockResolvedValue(listed("gpt-4o", "gpt-4o-mini"));
    await act(async () => button("Test connection").click());
    const boxes = () => host.querySelectorAll<HTMLInputElement>('.model-pick > ul > li input[type="checkbox"]');
    act(() => boxes()[0]!.click());
    act(() => boxes()[1]!.click());
    expect(save().disabled).toBe(true);
    expect(host.textContent).toContain("Assign this model to an agent: gpt-4o-mini.");
    act(() => button("Assign to agents… (gpt-4o-mini)").click());
    act(() => host.querySelector<HTMLButtonElement>('.agent-menu button[aria-label="General: fallback"]')!.click());
    expect(save().disabled).toBe(false);
    saveProvider.mockResolvedValue({ ...STATE, agent_models: {} });
    saveAgentModels.mockResolvedValue(STATE);
    await act(async () => save().click());
    expect(saveAgentModels).toHaveBeenCalledWith({
      agent_models: { general: { model: "openai::gpt-4o", fallbacks: ["openai::gpt-4o-mini"] } },
      default_model: "openai::gpt-4o",
    });
  });
});

describe("a saved provider", () => {
  const SAVED: ProvidersState = {
    ...STATE,
    providers: [
      ...STATE.providers,
      {
        id: "openai",
        name: "OpenAI (ChatGPT)",
        preset: "openai",
        kind: "openai",
        base_url: "https://openai.example/v1",
        key: "stored",
        models: [m("gpt-4o"), m("gpt-3.5", { unavailable: true, missing_since: 1 }), m("mine", { custom: true, context: 8000 })],
      },
    ],
    agent_models: { general: { model: "openai::gpt-4o", fallbacks: ["openai::gpt-3.5"] }, coder: { model: "openai::mine", fallbacks: [] } },
    default_model: "openai::gpt-4o",
  };
  const edit = () =>
    act(() =>
      root.render(
        <ProviderForm key="edit" state={SAVED} agents={AGENTS} edit={SAVED.providers[1]} onSaved={saved} onState={changed} />,
      ),
    );
  const changed = vi.fn();
  const ids = () => [...host.querySelectorAll(".model-pick > ul > li .mono")].map((e) => e.textContent);

  test("lists from the server's cache, with no refresh, and marks what the provider dropped", async () => {
    providerModels.mockResolvedValue({ models: [m("gpt-4o", { context: 128000 }), m("o3")], fetched_ms: Date.now() - 3 * 3_600_000 });
    await act(async () => edit());
    expect(providerModels).toHaveBeenCalledWith("openai");
    expect(refreshProviderModels).not.toHaveBeenCalled();
    expect(testProvider).not.toHaveBeenCalled();
    expect(ids()).toEqual(["gpt-3.5", "mine", "gpt-4o", "o3"]);
    expect(host.textContent).toContain("Listed 3 h ago");
    const row = (id: string) => [...host.querySelectorAll(".model-pick > ul > li")].find((li) => li.querySelector(".mono")!.textContent === id)!;
    expect(row("gpt-3.5").querySelector(".tag-unavailable")).not.toBeNull();
    expect(row("mine").querySelector(".tag-custom")).not.toBeNull();
    expect(row("mine").querySelector(".tag-unavailable")).toBeNull();
    // The cache's parameters, not the saved ones.
    expect(row("gpt-4o").querySelector(".tag-ctx")!.textContent).toBe("128k ctx");
    // Unchanged: saving needs no new test.
    expect(save().disabled).toBe(false);

    refreshProviderModels.mockResolvedValue({ models: [m("gpt-4o"), m("gpt-3.5")], fetched_ms: Date.now() });
    await act(async () => button("Refresh model list").click());
    expect(refreshProviderModels).toHaveBeenCalledWith("openai");
    expect(row("gpt-3.5").querySelector(".tag-unavailable")).toBeNull();
    expect(host.textContent).toContain("Listed just now");
  });

  test("an empty cache lists once", async () => {
    providerModels.mockResolvedValue({ models: [], fetched_ms: null });
    refreshProviderModels.mockResolvedValue({ models: [m("gpt-4o")], fetched_ms: Date.now() });
    await act(async () => edit());
    expect(refreshProviderModels).toHaveBeenCalledTimes(1);
  });

  test("the admin removes a dropped model from the provider and its agents", async () => {
    providerModels.mockResolvedValue({ models: [m("gpt-4o")], fetched_ms: Date.now() });
    await act(async () => edit());
    const after = { ...SAVED, agent_models: { general: { model: "openai::gpt-4o", fallbacks: [] } } };
    removeProviderModel.mockResolvedValue(after);
    await act(async () => button("Remove gpt-3.5 from this provider and its agents").click());
    expect(removeProviderModel).toHaveBeenCalledWith("openai", "gpt-3.5");
    expect(changed).toHaveBeenCalledWith(after);
    expect(ids()).not.toContain("gpt-3.5");
    expect([...host.querySelectorAll(".chain-fallbacks .chain-model")]).toHaveLength(0);
  });

  test("removing an agent's gone main model promotes its first fallback, as the server does", async () => {
    const state = { ...SAVED, agent_models: { general: { model: "openai::gpt-3.5", fallbacks: ["openai::gpt-4o"] }, coder: { model: "openai::mine", fallbacks: [] } } };
    providerModels.mockResolvedValue({ models: [m("gpt-4o")], fetched_ms: Date.now() });
    await act(async () =>
      root.render(<ProviderForm key="edit" state={state} agents={AGENTS} edit={state.providers[1]} onSaved={saved} onState={changed} />),
    );
    removeProviderModel.mockResolvedValue({ ...state, agent_models: { ...state.agent_models, general: { model: "openai::gpt-4o", fallbacks: [] } } });
    await act(async () => button("Remove gpt-3.5 from this provider and its agents").click());
    expect(host.textContent).not.toContain("General needs a main model");
    expect(button("Remove gpt-4o as General's main model")).toBeDefined();
    expect(save().disabled).toBe(false);
  });

  test("can be left with no chat models (kept for memory only)", async () => {
    providerModels.mockResolvedValue({ models: [m("gpt-4o")], fetched_ms: Date.now() });
    const bare = { ...SAVED, agent_models: { general: { model: "groq::llama", fallbacks: [] } }, default_model: "groq::llama" };
    await act(async () =>
      root.render(<ProviderForm key="edit" state={bare} agents={AGENTS} edit={bare.providers[1]} onSaved={saved} onState={changed} />),
    );
    act(() => button("Deselect all").click());
    expect(host.textContent).toContain("0 of");
    expect(save().disabled).toBe(false);
    saveProvider.mockResolvedValue(bare);
    await act(async () => save().click());
    expect(saveProvider).toHaveBeenCalledWith("openai", expect.objectContaining({ models: [] }));
  });

  test("a refused plan still tells the parent the provider was saved", async () => {
    changed.mockReset();
    providerModels.mockResolvedValue({ models: [m("gpt-4o"), m("o3")], fetched_ms: Date.now() });
    await act(async () => edit());
    act(() => [...host.querySelectorAll<HTMLInputElement>('.model-pick > ul > li input[type="checkbox"]')].at(-1)!.click());
    act(() => button("Assign to agents… (o3)").click());
    act(() => host.querySelector<HTMLButtonElement>('.agent-menu button[aria-label="Coder: fallback"]')!.click());
    const after = { ...SAVED, providers: [...SAVED.providers] };
    saveProvider.mockResolvedValue(after);
    saveAgentModels.mockRejectedValue(new Error("coder: at most 8 fallbacks"));
    await act(async () => save().click());
    expect(changed).toHaveBeenCalledWith(after);
    expect(saved).not.toHaveBeenCalled();
    expect(host.textContent).toContain("coder: at most 8 fallbacks");
  });
});

describe("countdown", () => {
  test("the first render with a deadline already counts", () => {
    const seen: number[] = [];
    function Probe(p: { until: number | null }) {
      seen.push(useCountdown(p.until));
      return null;
    }
    act(() => root.render(<Probe until={null} />));
    expect(seen).toEqual([0]);
    act(() => root.render(<Probe until={Date.now() + 10_000} />));
    // Before, the deadline's first render still read 0: Save was open for a frame.
    expect(seen.slice(1)).not.toContain(0);
    expect(seen.at(-1)).toBe(10);
  });
});

// @vitest-environment happy-dom
// Onboarding keeps every step as it was left: fill the provider, go on, come
// back, and the URL, the chosen models, the picker's search and the agents
// are all still there; the same for memory's choices.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import type { Agent, MemoryCfg, ProviderModel, ProvidersState, ProviderState } from "../types";

const api = {
  providers: vi.fn(),
  agents: vi.fn(),
  testProvider: vi.fn(),
  saveProvider: vi.fn(),
  saveAgentModels: vi.fn(),
  providerModels: vi.fn(),
  refreshProviderModels: vi.fn(),
  memoryCfg: vi.fn(),
  saveMemoryCfg: vi.fn(),
  saveSettings: vi.fn(),
  demoProvider: vi.fn(),
};
vi.mock("../api", () => ({ api }));
vi.mock("../apiPages", () => ({ pageApi: api }));
vi.mock("./forms", () => ({ ProfileFields: () => <div>profile fields</div>, SearchEngineFields: () => <div>engines</div> }));

const { default: Onboarding } = await import("./Onboarding");

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const m = (id: string): ProviderModel => ({
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
});
const agent = (id: string, name: string): Agent => ({ id, name, description: "", builtin: true, default_model: null, main_model: null, fallbacks: [], unavailable: [], avatar: { shape: "circle", eyes: "round", accent: "blue" } });
const STATE: ProvidersState = {
  providers: [],
  presets: [
    {
      id: "custom_openai",
      label: "OpenAI-compatible (custom)",
      kind: "openai",
      base_url: null,
      url_editable: true,
      needs_key: false,
      warning: null,
      custom: true,
      group: "own",
    },
  ],
  default_model: null,
  agent_models: {},
  configured: false,
};
const CFG: MemoryCfg = {
  backend: "builtin",
  recall: "keywords",
  source: "ollama",
  provider_id: null,
  embeddings_url: "",
  embeddings_model: "",
  preset: "balanced",
  model: "nomic-embed-text",
  active_model: null,
  presets: [{ id: "balanced", model: "nomic-embed-text", size: "274 MB" }],
};
const omni: ProviderState = {
  id: "omniroute",
  name: "Omniroute",
  preset: "custom_openai",
  kind: "openai",
  base_url: "https://api.omniroute.dev/v1",
  key: "none",
  models: [m("gpt-4o"), m("gpt-4o-mini")],
};

let host: HTMLElement;
let root: Root;

beforeEach(async () => {
  for (const f of Object.values(api)) f.mockReset();
  api.providers.mockResolvedValue(STATE);
  api.agents.mockResolvedValue({ agents: [agent("general", "General"), agent("coder", "Coder")], default: "general" });
  api.memoryCfg.mockResolvedValue(CFG);
  api.providerModels.mockResolvedValue({ models: [m("gpt-4o"), m("gpt-4o-mini"), m("o3")], fetched_ms: 1 });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () =>
    root.render(<Onboarding me={{ user_id: "u", username: "admin", role: "admin", exec_ceiling: "host" as const, must_change_password: false }} prefs={{ search_url: "", default_agent: null, default_model: null }} onDone={() => {}} />),
  );
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

const visible = () => host.querySelector<HTMLElement>(".onboard-step:not([hidden])")!;
const button = (label: string) =>
  [...visible().querySelectorAll<HTMLButtonElement>("button")].find((b) => b.textContent!.trim() === label || b.getAttribute("aria-label") === label)!;
const field = (label: string) => [...visible().querySelectorAll("label")].find((l) => l.textContent!.startsWith(label))!.querySelector("input")!;
const type = (input: HTMLInputElement, value: string) =>
  act(() => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, value);
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
const boxes = () => [...visible().querySelectorAll<HTMLInputElement>('.model-pick > ul > li input[type="checkbox"]')];

describe("onboarding", () => {
  test("every step is as it was left when the admin comes back", async () => {
    expect(visible().querySelector("h2")!.textContent).toBe("Connect an AI provider");
    type(field("API endpoint"), "https://api.omniroute.dev/v1");
    api.testProvider.mockResolvedValue({ ok: true, models: [m("gpt-4o"), m("gpt-4o-mini"), m("o3")] });
    await act(async () => button("Test connection").click());
    act(() => boxes()[0]!.click());
    act(() => boxes()[1]!.click());
    act(() => button("Assign to agents… (gpt-4o-mini)").click());
    act(() => host.querySelector<HTMLButtonElement>('.agent-menu button[aria-label="Coder: main model"]')!.click());
    type(visible().querySelector<HTMLInputElement>('input[type="search"]')!, "gpt");

    const saved = { ...STATE, providers: [omni], configured: true };
    api.saveProvider.mockResolvedValue(saved);
    api.saveAgentModels.mockResolvedValue({
      ...saved,
      agent_models: { general: { model: "omniroute::gpt-4o", fallbacks: [] }, coder: { model: "omniroute::gpt-4o-mini", fallbacks: [] } },
    });
    await act(async () => button("Save and continue").click());
    expect(visible().querySelector("h2")!.textContent).toBe("Memory");

    // Memory: pick meaning from Ollama, save, and come back to it later.
    act(() => button("Meaning").click());
    expect(button("Omniroute").getAttribute("aria-checked")).toBe("true");
    act(() => button("Ollama").click());
    act(() => button("Back").click());

    // The provider step as it was left.
    expect(visible().querySelector("h2")!.textContent).toBe("Connect an AI provider");
    expect(field("API endpoint").value).toBe("https://api.omniroute.dev/v1");
    expect(field("Name").value).toBe("Omniroute");
    expect(visible().querySelector<HTMLInputElement>('input[type="search"]')!.value).toBe("gpt");
    expect(boxes().map((b) => b.checked)).toEqual([true, true]);
    expect(button("For: Coder (gpt-4o-mini)")).toBeDefined();
    // Saved, so edited from now on: its prefix is not "taken".
    expect(visible().textContent).not.toContain("is taken");
    // Saved once, it goes on through Save and continue as it stands: no
    // separate way past it, and no demo skip.
    expect(button("Continue without saving")).toBeUndefined();
    expect(button("Skip: use the built-in demo (no real AI) for now")).toBeUndefined();
    expect(button("Save and continue").disabled).toBe(false);
    await act(async () => button("Save and continue").click());
    expect(visible().querySelector("h2")!.textContent).toBe("Memory");
    expect(button("Meaning").getAttribute("aria-checked")).toBe("true");
    expect(button("Ollama").getAttribute("aria-checked")).toBe("true");
    // Loaded once, when first shown.
    expect(api.memoryCfg).toHaveBeenCalledTimes(1);
  });
});

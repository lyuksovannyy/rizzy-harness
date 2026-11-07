// @vitest-environment happy-dom
// Adding a provider: a custom endpoint's name and prefix follow its URL
// until edited, and a preset with a warning keeps Save locked for its wait
// after a successful test, counting down.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import type { ProviderPreset, ProvidersState } from "../types";

const testProvider = vi.fn();
const saveProvider = vi.fn();
const saveAgentModels = vi.fn();
vi.mock("../api", () => ({ api: { testProvider, saveProvider, saveAgentModels } }));

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
  ],
  default_model: null,
  agent_models: {},
  configured: false,
};
const AGENTS = [
  { id: "general", name: "General", description: "", builtin: true, default_model: null },
  { id: "coder", name: "Coder", description: "", builtin: true, default_model: null },
];

let host: HTMLElement;
let root: Root;
const saved = vi.fn();

beforeEach(() => {
  vi.useFakeTimers();
  testProvider.mockReset();
  saveProvider.mockReset();
  saveAgentModels.mockReset();
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

const button = (label: string) => [...host.querySelectorAll<HTMLButtonElement>("button")].find((b) => b.textContent === label)!;
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
    expect(field("API endpoint").readOnly).toBe(true);
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
      models: [{ id: "meta/llama-3.1-70b-instruct", name: null, context: 131072, max_output: null, effort: null, vision: false, tools: true }],
    });
    expect(saveAgentModels).toHaveBeenCalledWith({
      agent_models: { general: "nvidia_nim::meta/llama-3.1-70b-instruct" },
      default_model: "nvidia_nim::meta/llama-3.1-70b-instruct",
    });
    expect(saved).toHaveBeenCalledWith(STATE);
  });

  test("a failed test keeps Save locked; changing the key asks for a new test", async () => {
    testProvider.mockResolvedValue({ ok: false, error: "401 Unauthorized" });
    await act(async () => button("Test connection").click());
    expect(host.textContent).toContain("401 Unauthorized");
    expect(save().disabled).toBe(true);
    testProvider.mockResolvedValue({ ok: true, models: [{ id: "m", name: null, context: null, max_output: null, effort: null, vision: null, tools: null }] });
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
    testProvider.mockResolvedValue({ ok: true, models: [{ id: "gpt-4o", name: null, context: null, max_output: null, effort: null, vision: null, tools: null }] });
    await act(async () => button("Test connection").click());
    act(() => host.querySelector<HTMLInputElement>('li input[type="checkbox"]')!.click());
    expect(save().textContent).toBe("Save provider");
    expect(save().disabled).toBe(false);
  });
});

const mount = (state: ProvidersState) =>
  act(() => root.render(<ProviderForm key={JSON.stringify(state)} state={state} agents={AGENTS} onSaved={saved} />));
const listed = (id: string) => ({ ok: true, models: [{ id, name: null, context: null, max_output: null, effort: null, vision: null, tools: null }] });

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
      providers: [{ id: "openai", name: "DeepSeek", preset: "custom_openai", kind: "openai", base_url: "https://api.deepseek.com", key: "stored", models: [{ id: "deepseek-chat", name: null, context: null, max_output: null, effort: null, vision: null, tools: null }] }],
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
  });
});

describe("General's default", () => {
  test("a default set before agent defaults existed is kept when another provider is added", async () => {
    mount({ ...STATE, default_model: "taken::deepseek-chat", agent_models: {} });
    choose("openai");
    testProvider.mockResolvedValue(listed("gpt-4o"));
    await act(async () => button("Test connection").click());
    act(() => host.querySelector<HTMLInputElement>('li input[type="checkbox"]')!.click());
    saveProvider.mockResolvedValue({ ...STATE, default_model: "taken::deepseek-chat", agent_models: {} });
    saveAgentModels.mockResolvedValue(STATE);
    await act(async () => save().click());
    expect(saveAgentModels).toHaveBeenCalledWith({ agent_models: { general: "taken::deepseek-chat" } });
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
    expect(saveAgentModels).toHaveBeenCalledWith({ agent_models: { general: "openai::gpt-4o" }, default_model: "openai::gpt-4o" });
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

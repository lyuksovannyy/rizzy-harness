// @vitest-environment happy-dom
// The admin's recall settings: a provider's embeddings models come from the
// server's cached listing, and one the listing leaves out is added by hand
// in the picker (there is no free-text field). Switching provider drops the
// previous provider's model. Custom registers a new provider (tested before
// it can be saved) and memory then uses it.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import type { MemoryCfg, ProviderModel, ProviderPreset, ProvidersState, ProviderState } from "../types";

const memoryCfg = vi.fn();
const providers = vi.fn();
const testProvider = vi.fn();
const saveMemoryCfg = vi.fn();
const providerModels = vi.fn();
const refreshProviderModels = vi.fn();
const saveProvider = vi.fn();
const testModel = vi.fn();
vi.mock("../apiPages", () => ({
  pageApi: { memoryCfg, providers, testProvider, saveMemoryCfg, providerModels, refreshProviderModels, saveProvider, testModel },
}));

const { MemoryCfgForm } = await import("./Memory");

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const CFG: MemoryCfg = {
  backend: "builtin",
  recall: "meaning",
  source: "provider",
  provider_id: "openai",
  embeddings_url: "",
  embeddings_model: "text-embedding-3-small",
  preset: "balanced",
  model: "text-embedding-3-small",
  active_model: "text-embedding-3-small",
  presets: [{ id: "balanced", model: "nomic-embed-text", size: "274 MB" }],
};
const provider = (id: string): ProviderState => ({
  id,
  name: id,
  preset: id,
  kind: "openai",
  base_url: `https://${id}.example/v1`,
  key: "stored",
  models: [],
});
const chat = (id: string): ProviderModel => ({
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
const custom: ProviderPreset = {
  id: "custom_openai",
  label: "OpenAI-compatible (custom)",
  kind: "openai",
  base_url: null,
  url_editable: true,
  needs_key: false,
  warning: null,
  custom: true,
  group: "own",
};
const STATE: ProvidersState = {
  providers: [provider("openai"), provider("mistral")],
  presets: [custom],
  default_model: null,
  agent_models: {},
  configured: true,
};

let host: HTMLElement;
let root: Root;

beforeEach(async () => {
  for (const f of [memoryCfg, providers, testProvider, saveMemoryCfg, providerModels, refreshProviderModels, saveProvider, testModel]) f.mockReset();
  memoryCfg.mockResolvedValue(CFG);
  providers.mockResolvedValue(STATE);
  // Only chat models listed: the embeddings model is one added by hand.
  providerModels.mockResolvedValue({ models: [chat("gpt-4o"), chat("gpt-4o-mini")], fetched_ms: 1 });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root.render(<MemoryCfgForm />));
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

const radios = () => [...host.querySelectorAll<HTMLInputElement>('.model-pick input[type="radio"]')];
const checked = () => radios().find((r) => r.checked)?.closest("li")?.querySelector(".mono")?.textContent;
const button = (label: string) =>
  [...host.querySelectorAll<HTMLButtonElement>("button")].find((b) => b.textContent!.trim() === label || b.getAttribute("aria-label") === label)!;
const seg = (label: string) => act(() => button(label).click());
const save = () => [...host.querySelectorAll<HTMLButtonElement>("button.primary")].at(-1)!;
const type = (input: HTMLInputElement, value: string) =>
  act(() => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, value);
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
const field = (label: string) =>
  [...host.querySelectorAll("label")].find((l) => l.textContent!.startsWith(label) && !l.closest("[hidden]"))!.querySelector("input")!;

describe("embeddings model", () => {
  test("the provider's cached listing, the saved model kept even when it leaves it out", () => {
    expect(providerModels).toHaveBeenCalledWith("openai");
    expect(testProvider).not.toHaveBeenCalled();
    expect(refreshProviderModels).not.toHaveBeenCalled();
    expect(checked()).toBe("text-embedding-3-small");
    expect(host.textContent).not.toContain("Or type a model id");
    expect(host.querySelectorAll('input:not([type="radio"]):not([type="search"])')).toHaveLength(0);
  });

  test("another provider starts with no model", async () => {
    await act(async () => seg("mistral"));
    expect(providerModels).toHaveBeenLastCalledWith("mistral");
    expect(checked()).toBeUndefined();
    expect(save().disabled).toBe(true);
  });

  test("a model added by hand is chosen and saved", async () => {
    await act(async () => seg("mistral"));
    act(() => button("Add custom model").click());
    type(field("Model id"), "mistral-embed");
    testModel.mockResolvedValue({ ok: true, reply: "1024 dimensions", ms: 80 });
    await act(async () => button("Test").click());
    expect(testModel).toHaveBeenCalledWith({
      provider_id: "mistral",
      preset: "mistral",
      base_url: "https://mistral.example/v1",
      kind: "openai",
      model: "mistral-embed",
    });
    act(() => button("Add model").click());
    expect(checked()).toBe("mistral-embed");
    saveMemoryCfg.mockResolvedValue({ ...CFG, provider_id: "mistral", active_model: "mistral-embed" });
    await act(async () => save().click());
    expect(saveMemoryCfg).toHaveBeenCalledWith({
      recall: "meaning",
      source: "provider",
      preset: "balanced",
      provider_id: "mistral",
      embeddings_model: "mistral-embed",
    });
  });
});

describe("custom", () => {
  test("registers a provider after a successful test, and memory uses it", async () => {
    await act(async () => seg("Custom"));
    // The provider form, for embeddings: no agents, Save after a test.
    expect(host.querySelector(".agent-pick")).toBeNull();
    const url = field("API endpoint");
    type(url, "https://api.embedder.dev/v1");
    expect(field("Name").value).toBe("Embedder");
    testProvider.mockResolvedValue({ ok: true, models: [chat("chat-1"), chat("embed-small")] });
    expect(save().disabled).toBe(true);
    await act(async () => button("Test connection").click());
    expect(testProvider).toHaveBeenCalledWith({ preset: "custom_openai", base_url: "https://api.embedder.dev/v1", api_key: undefined, provider_id: undefined });
    // Embeddings models first; one model only.
    expect(radios().map((r) => r.closest("li")!.querySelector(".mono")!.textContent)).toEqual(["embed-small", "chat-1"]);
    expect(save().disabled).toBe(true);
    act(() => radios()[0]!.click());
    expect(save().disabled).toBe(false);

    const after = { ...STATE, providers: [...STATE.providers, provider("embedder")] };
    saveProvider.mockResolvedValue(after);
    saveMemoryCfg.mockResolvedValue({ ...CFG, provider_id: "embedder", active_model: "embed-small" });
    await act(async () => save().click());
    expect(saveProvider).toHaveBeenCalledWith("embedder", {
      name: "Embedder",
      preset: "custom_openai",
      base_url: "https://api.embedder.dev/v1",
      api_key: undefined,
      models: [],
    });
    expect(saveMemoryCfg).toHaveBeenCalledWith({
      recall: "meaning",
      source: "provider",
      provider_id: "embedder",
      embeddings_model: "embed-small",
      preset: "balanced",
    });
    expect(host.textContent).toContain("Using embed-small.");
    expect(button("embedder").getAttribute("aria-checked")).toBe("true");
  });
});

describe("ollama", () => {
  test("presets, and Custom picks from Ollama's own listing", async () => {
    vi.useFakeTimers();
    try {
      await act(async () => seg("Ollama"));
      expect(host.querySelector(".model-pick")).toBeNull();
      expect(save().disabled).toBe(false);
      testProvider.mockResolvedValue({ ok: true, models: [chat("llama3"), chat("nomic-embed-text")] });
      const pick = [...host.querySelectorAll<HTMLButtonElement>('[aria-label="embeddings model preset"] button')].at(-1)!;
      expect(pick.textContent).toBe("Custom");
      await act(async () => pick.click());
      await act(async () => vi.advanceTimersByTime(500));
      expect(testProvider).toHaveBeenCalledWith({ preset: "ollama", base_url: "http://localhost:11434/v1" });
      expect(radios().map((r) => r.closest("li")!.querySelector(".mono")!.textContent)).toEqual(["nomic-embed-text", "llama3"]);
      expect(save().disabled).toBe(true);
    } finally {
      vi.useRealTimers();
    }
  });
});

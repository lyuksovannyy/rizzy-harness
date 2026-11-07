// @vitest-environment happy-dom
// The admin's recall settings: an embeddings model id can always be typed,
// even when the endpoint lists models, and switching provider drops the
// previous provider's model.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import type { MemoryCfg, ProviderState } from "../types";

const memoryCfg = vi.fn();
const providers = vi.fn();
const testProvider = vi.fn();
const saveMemoryCfg = vi.fn();
vi.mock("../api", () => ({ api: { memoryCfg, providers, testProvider, saveMemoryCfg } }));

const { MemoryCfgForm } = await import("./Memory");

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const CFG: MemoryCfg = {
  backend: "builtin",
  recall: "meaning",
  source: "provider",
  provider_id: "openai",
  embeddings_url: "",
  embeddings_model: "text-embedding-3-small",
  api_style: "openai",
  key: "none",
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
const chat = (id: string) => ({ id, name: null, context: null, max_output: null, effort: null, vision: null, tools: null });

let host: HTMLElement;
let root: Root;

beforeEach(async () => {
  memoryCfg.mockResolvedValue(CFG);
  providers.mockResolvedValue({ providers: [provider("openai"), provider("mistral")] });
  // Only chat models listed: the embeddings model must still be typeable.
  testProvider.mockResolvedValue({ ok: true, models: [chat("gpt-4o"), chat("gpt-4o-mini")] });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root.render(<MemoryCfgForm />));
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

const typed = () =>
  [...host.querySelectorAll("label")].find((l) => l.textContent!.startsWith("Or type a model id"))?.querySelector("input");

describe("embeddings model", () => {
  test("a listing that omits it still lets the id be typed", () => {
    expect(host.querySelector('input[type="radio"]')).not.toBeNull();
    expect(typed()?.value).toBe("text-embedding-3-small");
  });

  test("another provider starts with no model", async () => {
    const select = [...host.querySelectorAll("label")].find((l) => l.textContent!.startsWith("Provider"))!.querySelector("select")!;
    await act(async () => {
      select.value = "mistral";
      select.dispatchEvent(new Event("change", { bubbles: true }));
    });
    expect(typed()?.value).toBe("");
    expect(host.querySelector<HTMLButtonElement>("button.primary")!.disabled).toBe(true);
  });
});

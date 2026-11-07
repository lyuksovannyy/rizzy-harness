// A default model whose provider was removed is saved as the server default,
// which is what the select shows; the server would refuse it as unknown.
import { describe, expect, test } from "vitest";
import type { ModelInfo, Prefs } from "../types";
import { savablePrefs } from "./models";

const info = (id: string): ModelInfo => ({
  id,
  raw: id.split("::")[1]!,
  provider: "OpenAI",
  provider_id: id.split("::")[0]!,
  name: null,
  context: null,
  max_output: null,
  effort: false,
  vision: false,
  tools: false,
  default: false,
});
const MODELS = [info("openai::gpt-4o")];
const prefs = (default_model: string | null) => ({ default_model }) as Prefs;

describe("savable prefs", () => {
  test("a removed provider's model goes as the server default", () => {
    expect(savablePrefs(prefs("groq::llama-3"), MODELS).default_model).toBeNull();
  });
  test("known and legacy raw ids are kept", () => {
    expect(savablePrefs(prefs("openai::gpt-4o"), MODELS).default_model).toBe("openai::gpt-4o");
    expect(savablePrefs(prefs("gpt-4o"), MODELS).default_model).toBe("gpt-4o");
  });
  test("nothing is dropped before the models are known", () => {
    expect(savablePrefs(prefs("groq::llama-3"), []).default_model).toBe("groq::llama-3");
  });
});

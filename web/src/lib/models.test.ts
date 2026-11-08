// A default model whose provider was removed is saved as the server default,
// which is what the select shows; the server would refuse it as unknown.
// Agent plans: one main model per agent, fallbacks in order, and what keeps
// a plan from being saved.
import { describe, expect, test } from "vitest";
import type { ModelInfo, Prefs } from "../types";
import { ago, dropModels, MAX_FALLBACKS, modelLabel, moveFallback, offeredModels, type Plan, planChanges, planFrom, planProblems, renameModel, rolesOf, savablePrefs, setMain, toggleFallback } from "./models";

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
  unavailable: false,
});
const MODELS = [info("openai::gpt-4o")];
const prefs = (default_model: string | null) => ({ default_model }) as Prefs;

describe("savable prefs", () => {
  test("a removed provider's model goes as the server default", () => {
    expect(savablePrefs(prefs("groq::llama-3"), MODELS).default_model).toBeNull();
  });
  test("a known id is kept; an unprefixed one is unknown", () => {
    expect(savablePrefs(prefs("openai::gpt-4o"), MODELS).default_model).toBe("openai::gpt-4o");
    expect(savablePrefs(prefs("gpt-4o"), MODELS).default_model).toBeNull();
  });
  test("nothing is dropped before the models are known", () => {
    expect(savablePrefs(prefs("groq::llama-3"), []).default_model).toBe("groq::llama-3");
  });
});

describe("agent plans", () => {
  const AGENTS = [
    { id: "general", name: "General" },
    { id: "coder", name: "Coder" },
  ];

  test("one main model per agent; a model can serve many", () => {
    let p = setMain({}, "general", "a");
    p = setMain(p, "coder", "a");
    p = setMain(p, "general", "b");
    expect(p).toEqual({ general: { model: "b", fallbacks: [] }, coder: { model: "a", fallbacks: [] } });
  });

  test("fallbacks never repeat the main model or each other", () => {
    let p = setMain({}, "general", "a");
    p = toggleFallback(p, "general", "b");
    p = toggleFallback(p, "general", "c");
    expect(p.general).toEqual({ model: "a", fallbacks: ["b", "c"] });
    // The main model made a fallback is no longer the main one.
    p = toggleFallback(p, "general", "a");
    expect(p.general).toEqual({ model: null, fallbacks: ["b", "c", "a"] });
    // A fallback made the main model leaves the fallbacks.
    p = setMain(p, "general", "c");
    expect(p.general).toEqual({ model: "c", fallbacks: ["b", "a"] });
    p = toggleFallback(p, "general", "b");
    expect(p.general).toEqual({ model: "c", fallbacks: ["a"] });
  });

  test("fallbacks reorder within bounds", () => {
    let p: Plan = { general: { model: "a", fallbacks: ["b", "c", "d"] } };
    p = moveFallback(p, "general", 2, -1);
    expect(p.general!.fallbacks).toEqual(["b", "d", "c"]);
    p = moveFallback(p, "general", 0, 1);
    expect(p.general!.fallbacks).toEqual(["d", "b", "c"]);
    expect(moveFallback(p, "general", 0, -1)).toBe(p);
    expect(moveFallback(p, "general", 2, 1)).toBe(p);
  });

  test("problems: unassigned models, General without a main, fallbacks without one", () => {
    expect(planProblems({}, ["a"], AGENTS)).toEqual([
      "Assign this model to an agent: a.",
      "General needs a main model: it is the default for everyone.",
    ]);
    const p: Plan = { general: { model: "a", fallbacks: [] }, coder: { model: null, fallbacks: ["b"] } };
    expect(planProblems(p, ["a", "b", "c"], AGENTS)).toEqual([
      "Assign this model to an agent: c.",
      "Coder has fallbacks but no main model.",
    ]);
    expect(planProblems({ general: { model: "a", fallbacks: ["b"] } }, ["a", "b"], AGENTS)).toEqual([]);
    // An agent the server does not know does not count.
    expect(planProblems({ general: { model: "x", fallbacks: [] }, gone: { model: "a", fallbacks: [] } }, ["a"], AGENTS)).toHaveLength(1);
  });

  test("problems when agents may inherit a chain: General may have none, fallbacks still need a main", () => {
    expect(planProblems({}, [], AGENTS, undefined, true)).toEqual([]);
    expect(planProblems({ general: { model: null, fallbacks: ["b"] } }, ["b"], AGENTS, undefined, true)).toEqual([
      "General has fallbacks but no main model.",
    ]);
  });

  test("a plan starts from the saved agents, this provider's models local, the demo's dropped", () => {
    const saved = {
      general: { model: "openai::gpt-4o", fallbacks: ["groq::llama", "demo::echo"] },
      coder: { model: "demo::echo", fallbacks: [] },
    };
    const p = planFrom(saved, null, "openai", (id) => id.startsWith("demo::"));
    expect(p).toEqual({ general: { model: "::gpt-4o", fallbacks: ["groq::llama"] }, coder: { model: null, fallbacks: [] } });
    // General without a chain of its own starts on the server default model.
    expect(planFrom({}, "groq::llama", "openai").general).toEqual({ model: "groq::llama", fallbacks: [] });
  });

  test("the changes to save: only what differs, local ids prefixed, empty agents removed", () => {
    const saved = { general: { model: "groq::llama", fallbacks: [] }, coder: { model: "groq::llama", fallbacks: [] } };
    const p: Plan = {
      general: { model: "groq::llama", fallbacks: [] },
      coder: { model: "::m1", fallbacks: ["::m2", "groq::llama"] },
      writer: { model: null, fallbacks: [] },
    };
    expect(planChanges(saved, p, "mine", ["general", "coder", "writer"])).toEqual({
      coder: { model: "mine::m1", fallbacks: ["mine::m2", "groq::llama"] },
    });
    expect(planChanges(saved, { general: { model: null, fallbacks: [] } }, "mine", ["general"])).toEqual({ general: null });
  });

  test("renaming and dropping reach main models and fallbacks", () => {
    const p: Plan = { general: { model: "::a", fallbacks: ["::b"] }, coder: { model: "::b", fallbacks: ["::a"] } };
    expect(renameModel(p, "::a", "::z")).toEqual({ general: { model: "::z", fallbacks: ["::b"] }, coder: { model: "::b", fallbacks: ["::z"] } });
    expect(dropModels(p, (id) => id !== "::b")).toEqual({ general: { model: "::a", fallbacks: [] }, coder: { model: null, fallbacks: ["::a"] } });
    expect(rolesOf(p, "::b")).toEqual([
      { agent: "general", role: "fallback" },
      { agent: "coder", role: "main" },
    ]);
  });

  test("dropping with promote puts the first fallback left in the main model's place", () => {
    const p: Plan = { general: { model: "::a", fallbacks: ["::b", "::c"] }, coder: { model: "::a", fallbacks: [] }, writer: { model: null, fallbacks: ["::c"] } };
    expect(dropModels(p, (id) => id !== "::a", true)).toEqual({
      general: { model: "::b", fallbacks: ["::c"] },
      coder: { model: null, fallbacks: [] },
      // A main model left empty on purpose stays empty.
      writer: { model: null, fallbacks: ["::c"] },
    });
  });

  test("no agent takes more fallbacks than the server keeps", () => {
    const many = Array.from({ length: MAX_FALLBACKS + 1 }, (_, i) => `f${i}`);
    expect(planProblems({ general: { model: "a", fallbacks: many.slice(0, MAX_FALLBACKS) } }, ["a", ...many.slice(0, MAX_FALLBACKS)], AGENTS)).toEqual([]);
    expect(planProblems({ general: { model: "a", fallbacks: many } }, ["a", ...many], AGENTS)).toEqual(["General has more than 8 fallbacks."]);
  });
});

describe("models the provider dropped", () => {
  test("are offered only when already chosen, and marked", () => {
    const gone = { ...info("openai::gpt-4"), unavailable: true };
    const models = [gone, info("openai::gpt-4o")];
    expect(offeredModels(models).map((m) => m.id)).toEqual(["openai::gpt-4o"]);
    expect(offeredModels(models, "openai::gpt-4").map((m) => m.id)).toEqual(["openai::gpt-4", "openai::gpt-4o"]);
    expect(modelLabel(gone, models)).toBe("gpt-4 (unavailable)");
  });
});

describe("ago", () => {
  test("rounds down to the largest unit", () => {
    const now = 10 * 86_400_000;
    expect(ago(now - 5_000, now)).toBe("just now");
    expect(ago(now - 5 * 60_000, now)).toBe("5 min ago");
    expect(ago(now - 3 * 3_600_000 - 1, now)).toBe("3 h ago");
    expect(ago(now - 86_400_000, now)).toBe("1 day ago");
    expect(ago(now - 3 * 86_400_000, now)).toBe("3 days ago");
  });
});

// @vitest-environment happy-dom
// The model picker: a search over ids and names, pages of 100, select and
// deselect all of what the search shows, a tag per reported parameter, one
// or several models, a refresh only on request, models added by hand (and
// tested), and agents: one main model each, fallbacks in order, and Save
// held until every chosen model serves one. Memory's listing puts
// embeddings models first.
import { act, useState } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { embedFirst, type Plan } from "../lib/models";
import type { Agent, ProviderModel } from "../types";

const testModel = vi.fn();
vi.mock("../api", () => ({ api: { testModel } }));

const { filterModels, modelTags, ModelPicker } = await import("./ModelPicker");

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const model = (id: string, more: Partial<ProviderModel> = {}): ProviderModel => ({
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
const agent = (id: string, name: string): Agent => ({
  id,
  name,
  description: "",
  builtin: true,
  default_model: null,
  main_model: null,
  fallbacks: [],
  unavailable: [],
  avatar: { shape: "circle", eyes: "round", accent: "blue" },
});
const AGENTS = [agent("general", "General"), agent("coder", "Coder")];

const MODELS = Array.from({ length: 250 }, (_, i) => model(`model-${String(i).padStart(3, "0")}`));
MODELS.push(model("gpt-4o", { name: "GPT-4o Omni", context: 128_000, max_output: 16_384, vision: true, tools: true }));

let host: HTMLElement;
let root: Root;
let selected: string[] = [];

function Harness() {
  const [sel, setSel] = useState<string[]>([]);
  selected = sel;
  return <ModelPicker label="Models" models={MODELS} selected={sel} onChange={setSel} />;
}

beforeEach(() => {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  act(() => root.render(<Harness />));
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

const rows = () => [...host.querySelectorAll(".model-pick > ul > li")].map((li) => li.querySelector(".mono")!.textContent);
const button = (label: string) =>
  [...host.querySelectorAll<HTMLButtonElement>("button")].find((b) => b.textContent!.trim() === label || b.getAttribute("aria-label") === label)!;
const search = (q: string) =>
  act(() => {
    const input = host.querySelector<HTMLInputElement>('input[type="search"]')!;
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, q);
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });

describe("model picker", () => {
  test("pages of 100", () => {
    expect(rows()).toHaveLength(100);
    expect(host.textContent).toContain("1–100 of 251");
    expect(button("Previous page").disabled).toBe(true);
    act(() => button("Next page").click());
    act(() => button("Next page").click());
    expect(rows()).toHaveLength(51);
    expect(host.textContent).toContain("201–251 of 251");
    expect(button("Next page").disabled).toBe(true);
  });

  test("a search goes back to the first page and matches names too", () => {
    act(() => button("Next page").click());
    search("model-01");
    expect(rows()).toEqual(Array.from({ length: 10 }, (_, i) => `model-01${i}`));
    expect(host.querySelector(".pager")).toBeNull();
    search("omni");
    expect(rows()).toEqual(["gpt-4o"]);
  });

  test("select and deselect all act on the search", () => {
    act(() => host.querySelector<HTMLInputElement>('li input[type="checkbox"]')!.click());
    expect(selected).toEqual(["model-000"]);
    search("model-01");
    act(() => button("Select all").click());
    expect(selected).toHaveLength(11);
    expect(host.textContent).toContain("11 of 251 selected");
    act(() => button("Deselect all").click());
    expect(selected).toEqual(["model-000"]);
    search("");
    act(() => button("Select all").click());
    expect(selected).toHaveLength(251);
  });

  test("tags show what was reported, each kind with its own icon and accent", () => {
    const text = (m: ProviderModel) => modelTags(m).map((t) => `${t.kind}:${t.text}`);
    expect(text(MODELS.at(-1)!)).toEqual(["ctx:128k ctx", "out:16k out", "vision:vision", "tools:tools"]);
    expect(text(model("x", { context: 1_048_576, effort: true, vision: false }))).toEqual(["ctx:1M ctx", "effort:effort"]);
    expect(text(model("x"))).toEqual([]);
    expect(text(model("x", { custom: true, context: 8192 }))).toEqual(["custom:custom", "ctx:8k ctx"]);
    expect(text(model("x", { unavailable: true }))).toEqual(["unavailable:unavailable"]);
    // A model added by hand is never unavailable.
    expect(text(model("x", { custom: true, unavailable: true }))).toEqual(["custom:custom"]);
    search("gpt");
    const tags = [...host.querySelectorAll(".model-tag")];
    expect(tags.map((c) => c.className)).toEqual(["model-tag tag-ctx", "model-tag tag-out", "model-tag tag-vision", "model-tag tag-tools"]);
    expect(tags.map((c) => c.textContent)).toEqual(["128k ctx", "16k out", "vision", "tools"]);
    expect(tags.every((c) => c.querySelector("svg"))).toBe(true);
  });

  test("every word of the search must match", () => {
    expect(filterModels(MODELS, "gpt omni").map((m) => m.id)).toEqual(["gpt-4o"]);
    expect(filterModels(MODELS, "gpt claude")).toEqual([]);
    expect(filterModels(MODELS, "  ")).toBe(MODELS);
  });
});

describe("embeddings first", () => {
  test("embedding, then embed, then the rest, each by id", () => {
    const list = [
      model("mistral-large"),
      model("nomic-embed-text"),
      model("text-embedding-3-small"),
      model("e5", { name: "Multilingual embedding" }),
      model("all-minilm", { name: "MiniLM embed" }),
      model("codestral"),
    ];
    expect(list.sort(embedFirst).map((m) => m.id)).toEqual([
      "e5",
      "text-embedding-3-small",
      "all-minilm",
      "nomic-embed-text",
      "codestral",
      "mistral-large",
    ]);
  });
});

describe("inside a form", () => {
  test("Enter in the search does not submit it", () => {
    let submits = 0;
    act(() =>
      root.render(
        <form
          onSubmit={(e) => {
            e.preventDefault();
            submits++;
          }}
        >
          <ModelPicker label="Embeddings model" mode="single" models={MODELS} selected={[]} onChange={() => {}} />
          <button>Save</button>
        </form>,
      ),
    );
    const input = host.querySelector<HTMLInputElement>('input[type="search"]')!;
    const ev = new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true });
    act(() => void input.dispatchEvent(ev));
    expect(ev.defaultPrevented).toBe(true);
    expect(submits).toBe(0);
  });
});

describe("single and multi", () => {
  test("single: radios, one model at a time, no select all", () => {
    let sel: string[] = [];
    function One() {
      const [s, set] = useState<string[]>([]);
      sel = s;
      return <ModelPicker label="Embeddings model" mode="single" models={MODELS.slice(0, 3)} selected={s} onChange={set} />;
    }
    act(() => root.render(<One />));
    const radios = [...host.querySelectorAll<HTMLInputElement>('input[type="radio"]')];
    expect(radios).toHaveLength(3);
    expect(host.querySelector('input[type="checkbox"]')).toBeNull();
    expect(button("Select all")).toBeUndefined();
    act(() => radios[0]!.click());
    act(() => radios[2]!.click());
    expect(sel).toEqual(["model-002"]);
  });

  test("multi: checkboxes add up", () => {
    const boxes = () => [...host.querySelectorAll<HTMLInputElement>('li input[type="checkbox"]')];
    act(() => boxes()[0]!.click());
    act(() => boxes()[1]!.click());
    expect(selected).toEqual(["model-000", "model-001"]);
  });
});

describe("refresh", () => {
  test("only on request, and spinning while it runs", () => {
    const onRefresh = vi.fn();
    act(() => root.render(<ModelPicker label="Models" models={MODELS.slice(0, 2)} selected={[]} onChange={() => {}} onRefresh={onRefresh} />));
    expect(onRefresh).not.toHaveBeenCalled();
    act(() => button("Refresh model list").click());
    expect(onRefresh).toHaveBeenCalledTimes(1);
    act(() =>
      root.render(<ModelPicker label="Models" models={MODELS.slice(0, 2)} selected={[]} onChange={() => {}} onRefresh={onRefresh} refreshing />),
    );
    // Off while it lists, but focusable, so focus does not fall to the page.
    expect(button("Refresh model list").disabled).toBe(false);
    expect(button("Refresh model list").getAttribute("aria-disabled")).toBe("true");
    expect(button("Refresh model list").querySelector("svg")!.getAttribute("class")).toContain("spin");
    act(() => button("Refresh model list").click());
    expect(onRefresh).toHaveBeenCalledTimes(1);
  });

  test("no button without a way to list", () => {
    expect(button("Refresh model list")).toBeUndefined();
  });
});

describe("models added by hand", () => {
  const field = (label: string) =>
    [...host.querySelectorAll(".custom-model label")].find((l) => l.textContent!.startsWith(label))!.querySelector("input")!;
  const type = (input: HTMLInputElement, value: string) =>
    act(() => {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, value);
      input.dispatchEvent(new Event("input", { bubbles: true }));
    });

  test("added after a test, with its parameters; edited and removed", async () => {
    let list = MODELS.slice(0, 2);
    const onSave = vi.fn((m: ProviderModel, was: string | null) => {
      list = [...list.filter((x) => x.id !== was), m];
    });
    const onRemove = vi.fn();
    const draw = () =>
      act(() =>
        root.render(
          <ModelPicker
            label="Models"
            models={list}
            selected={[]}
            onChange={() => {}}
            custom={{ onSave, onRemove, test: () => ({ provider_id: "openai" }) }}
          />,
        ),
      );
    draw();
    act(() => button("Add custom model").click());
    // The form takes focus as it opens.
    expect(document.activeElement).toBe(field("Model id"));
    const add = () => [...host.querySelectorAll<HTMLButtonElement>(".custom-model button")].find((b) => b.textContent === "Add model")!;
    expect(add().disabled).toBe(true);
    type(field("Model id"), "model-000");
    expect(host.textContent).toContain("model-000 is already in the list.");
    type(field("Model id"), "my-model");
    type(field("Context window"), "lots");
    expect(add().disabled).toBe(true);
    type(field("Context window"), "200000");
    type(field("Max output"), "8192");
    act(() => [...host.querySelectorAll<HTMLInputElement>(".custom-model .check input")][2]!.click());

    testModel.mockResolvedValueOnce({ ok: true, reply: "OK.", ms: 420 });
    await act(async () => [...host.querySelectorAll<HTMLButtonElement>(".custom-model button")].find((b) => b.textContent === "Test")!.click());
    expect(testModel).toHaveBeenCalledWith({ provider_id: "openai", model: "my-model" });
    expect(host.textContent).toContain("Replied “OK.” in 420 ms.");
    testModel.mockResolvedValueOnce({ ok: false, error: "404 model not found" });
    await act(async () => [...host.querySelectorAll<HTMLButtonElement>(".custom-model button")].find((b) => b.textContent === "Test")!.click());
    expect(host.textContent).toContain("404 model not found");

    act(() => add().click());
    expect(onSave).toHaveBeenCalledWith(
      {
        id: "my-model",
        name: null,
        context: 200_000,
        max_output: 8192,
        effort: false,
        vision: false,
        tools: true,
        custom: true,
        unavailable: false,
        missing_since: null,
      },
      null,
    );
    expect(host.querySelector(".custom-model")).toBeNull();
    // Closed, focus goes back to what opened it.
    expect(document.activeElement).toBe(button("Add custom model"));
    draw();
    expect(host.querySelector(".tag-custom")).not.toBeNull();
    act(() => button("Edit my-model").click());
    expect(document.activeElement).toBe(field("Name"));
    act(() => [...host.querySelectorAll<HTMLButtonElement>(".custom-model button")].find((b) => b.textContent === "Cancel")!.click());
    expect(document.activeElement).toBe(button("Edit my-model"));
    act(() => button("Edit my-model").click());
    expect(field("Model id").value).toBe("my-model");
    expect(field("Context window").value).toBe("200000");
    type(field("Model id"), "my-model-2");
    act(() => [...host.querySelectorAll<HTMLButtonElement>(".custom-model button")].find((b) => b.textContent === "Save model")!.click());
    expect(onSave).toHaveBeenLastCalledWith(expect.objectContaining({ id: "my-model-2" }), "my-model");
    draw();
    act(() => button("Remove my-model-2").click());
    expect(onRemove).toHaveBeenCalledWith("my-model-2");
  });

  test("Enter in a field never submits, and still presses a button", () => {
    act(() =>
      root.render(
        <ModelPicker label="Models" models={[]} selected={[]} onChange={() => {}} custom={{ onSave: () => {}, onRemove: () => {}, test: null }} />,
      ),
    );
    act(() => button("Add custom model").click());
    const enter = (el: Element) => {
      const e = new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true });
      el.dispatchEvent(e);
      return e.defaultPrevented;
    };
    expect(enter(field("Model id"))).toBe(true);
    const cancel = [...host.querySelectorAll<HTMLButtonElement>(".custom-model button")].find((b) => b.textContent === "Cancel")!;
    expect(enter(cancel)).toBe(false);
  });

  test("no test before the provider can be reached", () => {
    act(() =>
      root.render(
        <ModelPicker label="Models" models={[]} selected={[]} onChange={() => {}} custom={{ onSave: () => {}, onRemove: () => {}, test: null }} />,
      ),
    );
    expect(host.textContent).toContain("No models listed. Add one by hand below.");
    act(() => button("Add custom model").click());
    expect([...host.querySelectorAll<HTMLButtonElement>(".custom-model button")].find((b) => b.textContent === "Test")!.disabled).toBe(true);
    expect(host.textContent).toContain("Test the connection first.");
  });

  test("an unavailable saved model can be removed", () => {
    const onRemoveUnavailable = vi.fn();
    act(() =>
      root.render(
        <ModelPicker
          label="Models"
          models={[model("old", { unavailable: true }), model("new")]}
          selected={["old"]}
          onChange={() => {}}
          onRemoveUnavailable={onRemoveUnavailable}
        />,
      ),
    );
    expect(host.querySelector(".tag-unavailable")!.textContent).toBe("unavailable");
    act(() => button("Remove old from this provider and its agents").click());
    expect(onRemoveUnavailable).toHaveBeenCalledWith("old");
    expect(button("Remove new from this provider and its agents")).toBeUndefined();
  });
});

describe("agents", () => {
  let plan: Plan = {};
  function Assign(p: { initial: Plan; models?: ProviderModel[]; sel?: string[] }) {
    const [sel, setSel] = useState<string[]>(p.sel ?? []);
    const [pl, setPl] = useState<Plan>(p.initial);
    selected = sel;
    plan = pl;
    return (
      <ModelPicker
        label="Models"
        models={p.models ?? MODELS.slice(0, 5)}
        selected={sel}
        onChange={setSel}
        agents={{ agents: AGENTS, plan: pl, onPlan: setPl, ref: (raw) => `::${raw}`, label: (id) => id.replace(/^::/, "") }}
      />
    );
  }
  const box = (i: number) => host.querySelectorAll<HTMLInputElement>('.model-pick > ul > li input[type="checkbox"]')[i]!;
  const item = (label: string) => host.querySelector<HTMLButtonElement>(`.agent-menu button[aria-label="${label}"]`)!;

  test("the first model chosen starts as General's; the rest wait to be assigned", () => {
    act(() => root.render(<Assign initial={{}} />));
    act(() => box(0).click());
    expect(plan.general).toEqual({ model: "::model-000", fallbacks: [] });
    act(() => box(1).click());
    expect(host.textContent).toContain("Assign this model to an agent: model-001.");
    expect(button("Assign to agents… (model-001)").className).toContain("loose");

    // Coder's main model, then a fallback of General's.
    act(() => button("Assign to agents… (model-001)").click());
    act(() => item("Coder: main model").click());
    expect(plan.coder).toEqual({ model: "::model-001", fallbacks: [] });
    act(() => item("General: fallback").click());
    expect(plan.general).toEqual({ model: "::model-000", fallbacks: ["::model-001"] });
    expect(host.querySelector('[role="status"]')).toBeNull();
    expect(button("For: General (fallback), Coder (model-001)")).toBeDefined();

    // Let go, a model leaves every agent it served.
    act(() => box(1).click());
    expect(plan).toEqual({ general: { model: "::model-000", fallbacks: [] }, coder: { model: null, fallbacks: [] } });
  });

  test("General without a main model is a problem", () => {
    act(() => root.render(<Assign initial={{ general: { model: "::model-000", fallbacks: [] } }} sel={["model-000"]} />));
    act(() => button("Remove model-000 as General's main model").click());
    expect(host.textContent).toContain("General needs a main model");
  });

  test("fallbacks move up and down and come off", () => {
    const initial: Plan = { general: { model: "::model-000", fallbacks: ["::model-001", "::model-002", "groq::llama"] } };
    act(() => root.render(<Assign initial={initial} sel={["model-000", "model-001", "model-002"]} />));
    expect(button("Move model-001 up in General's fallbacks").disabled).toBe(true);
    expect(button("Move groq::llama down in General's fallbacks").disabled).toBe(true);
    act(() => button("Move groq::llama up in General's fallbacks").click());
    expect(plan.general!.fallbacks).toEqual(["::model-001", "groq::llama", "::model-002"]);
    act(() => button("Move model-001 down in General's fallbacks").click());
    expect(plan.general!.fallbacks).toEqual(["groq::llama", "::model-001", "::model-002"]);
    act(() => button("Remove model-002 from General's fallbacks").click());
    expect(plan.general!.fallbacks).toEqual(["groq::llama", "::model-001"]);
    expect(host.textContent).toContain("Assign this model to an agent: model-002.");
    const chain = [...host.querySelectorAll(".chain-fallbacks .chain-model")].map((e) => e.textContent);
    expect(chain).toEqual(["groq::llama", "model-001"]);
  });

  test("focus stays on a fallback's buttons as it moves and comes off", () => {
    const initial: Plan = { general: { model: "::model-000", fallbacks: ["::model-001", "::model-002", "::model-003"] } };
    act(() => root.render(<Assign initial={initial} sel={["model-000", "model-001", "model-002", "model-003"]} />));
    const down = button("Move model-002 down in General's fallbacks");
    down.focus();
    act(() => down.click());
    expect(plan.general!.fallbacks).toEqual(["::model-001", "::model-003", "::model-002"]);
    // Down is off at the end, so focus takes Up of the same model.
    expect(document.activeElement).toBe(button("Move model-002 up in General's fallbacks"));
    act(() => (document.activeElement as HTMLButtonElement).click());
    expect(document.activeElement).toBe(button("Move model-002 up in General's fallbacks"));
    act(() => (document.activeElement as HTMLButtonElement).click());
    expect(plan.general!.fallbacks).toEqual(["::model-002", "::model-001", "::model-003"]);
    // Up is off at the top: Down of the same model.
    expect(document.activeElement).toBe(button("Move model-002 down in General's fallbacks"));
    // Taken off, focus goes to the next fallback's remove button, then the main model's.
    act(() => button("Remove model-002 from General's fallbacks").click());
    expect(document.activeElement).toBe(button("Remove model-001 from General's fallbacks"));
    act(() => button("Remove model-001 from General's fallbacks").click());
    act(() => button("Remove model-003 from General's fallbacks").click());
    expect(document.activeElement).toBe(button("Remove model-000 as General's main model"));
  });

  test("an agent takes at most 8 fallbacks", () => {
    const models = MODELS.slice(0, 10);
    const fallbacks = models.slice(1, 9).map((m) => `::${m.id}`);
    act(() => root.render(<Assign initial={{ general: { model: "::model-000", fallbacks } }} models={models} sel={models.map((m) => m.id)} />));
    act(() => button("Assign to agents… (model-009)").click());
    expect(item("General: fallback").getAttribute("aria-disabled")).toBe("true");
    act(() => item("General: fallback").click());
    expect(plan.general!.fallbacks).toHaveLength(8);
    // One of them can still come off from its own menu.
    act(() => item("General: fallback").dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true })));
    act(() => button("For: General (fallback) (model-008)").click());
    expect(item("General: fallback").getAttribute("aria-disabled")).toBeNull();
    act(() => item("General: fallback").click());
    expect(plan.general!.fallbacks).toHaveLength(7);
  });

  test("the menu closes when the list scrolls its button away", () => {
    act(() => root.render(<Assign initial={{}} sel={["model-000"]} />));
    act(() => button("Assign to agents… (model-000)").click());
    expect(host.querySelector(".agent-menu")).not.toBeNull();
    act(() => void host.querySelector(".model-pick > ul")!.dispatchEvent(new Event("scroll")));
    expect(host.querySelector(".agent-menu")).toBeNull();
  });
});

// @vitest-environment happy-dom
// The model picker: a search over ids and names, pages of 100, select and
// deselect all of what the search shows, and chips only for what the
// provider reported. Memory's listing puts embeddings models first.
import { act, useState } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test } from "vitest";
import type { ProviderModel } from "../types";
import { embedFirst } from "./Memory";
import { filterModels, modelChips, ModelPicker } from "./ModelPicker";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const model = (id: string, more: Partial<ProviderModel> = {}): ProviderModel => ({
  id,
  name: null,
  context: null,
  max_output: null,
  effort: null,
  vision: null,
  tools: null,
  ...more,
});

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

const rows = () => [...host.querySelectorAll("li")].map((li) => li.querySelector(".mono")!.textContent);
const button = (label: string) =>
  [...host.querySelectorAll<HTMLButtonElement>("button")].find((b) => b.textContent === label || b.getAttribute("aria-label") === label)!;
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

  test("chips show what was reported, nothing for unknowns", () => {
    expect(modelChips(MODELS.at(-1)!)).toEqual(["128k ctx", "16k out", "vision", "tools"]);
    expect(modelChips(model("x", { context: 1_048_576, effort: true, vision: false }))).toEqual(["1M ctx", "effort"]);
    expect(modelChips(model("x"))).toEqual([]);
    search("gpt");
    expect([...host.querySelectorAll(".model-chip")].map((c) => c.textContent)).toEqual(["128k ctx", "16k out", "vision", "tools"]);
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
          <ModelPicker label="Embeddings model" single models={MODELS} selected={[]} onChange={() => {}} />
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

describe("agent menu", () => {
  test("closes when the list scrolls its button away", () => {
    const agents = [{ id: "general", name: "General", description: "", builtin: true, default_model: null }];
    act(() =>
      root.render(
        <ModelPicker
          label="Models"
          models={MODELS.slice(0, 5)}
          selected={["model-000"]}
          onChange={() => {}}
          agents={{ agents, assigned: {}, onAssign: () => {} }}
        />,
      ),
    );
    act(() => button("Default agents for model-000: none").click());
    expect(host.querySelector(".agent-menu")).not.toBeNull();
    act(() => void host.querySelector("ul")!.dispatchEvent(new Event("scroll")));
    expect(host.querySelector(".agent-menu")).toBeNull();
  });
});

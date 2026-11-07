// @vitest-environment happy-dom
// Settings, Preferences: the default agent lists each agent with its face,
// or the red alert when its model is gone; the default model offers only
// listed models, keeping a gone current one, marked, so it can be changed.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import type { Agent, ModelInfo, Prefs } from "../types";

const saveSettings = vi.fn();
vi.mock("../api", () => ({ api: { saveSettings } }));

const { default: Settings } = await import("./Settings");

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLElement;
let root: Root;
beforeEach(() => {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
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
  tools: false,
  default: false,
  unavailable,
});
const agent = (id: string, name: string, more: Partial<Agent> = {}): Agent => ({
  id,
  name,
  description: "",
  builtin: true,
  default_model: null,
  main_model: null,
  fallbacks: [],
  unavailable: [],
  avatar: { shape: "circle", eyes: "round", accent: "blue" },
  ...more,
});

function render(prefs: Partial<Prefs>) {
  act(() =>
    root.render(
      <Settings
        me={{ user_id: "u", username: "alex", role: "member", can_agentic: false, must_change_password: false }}
        agents={[agent("general", "General"), agent("coder", "Coder", { main_model: "openai::gpt-4", unavailable: ["openai::gpt-4"] })]}
        models={[model("gpt-4", true), model("gpt-4o")]}
        prefs={{ search_url: "", default_agent: null, default_model: null, ...prefs } as Prefs}
        onPrefs={() => {}}
        onProviderChanged={() => {}}
        initialTab="preferences"
        memoryVersion={0}
        nodes={[]}
      />,
    ),
  );
}
const modelOptions = () =>
  [...[...host.querySelectorAll("label")].find((l) => l.textContent!.startsWith("Default model"))!.querySelectorAll("option")].map(
    (o) => [o.value, o.disabled, o.textContent],
  );

describe("Preferences", () => {
  test("the default model offers listed models only", () => {
    render({});
    expect(modelOptions()).toEqual([
      ["", false, "server default"],
      ["openai::gpt-4o", false, "gpt-4o"],
    ]);
  });

  test("a gone default stays shown, marked, until it is changed", () => {
    render({ default_model: "openai::gpt-4" });
    expect(modelOptions()).toEqual([
      ["", false, "server default"],
      ["openai::gpt-4", true, "gpt-4 (unavailable)"],
      ["openai::gpt-4o", false, "gpt-4o"],
    ]);
  });

  test("the default agent list marks an agent whose model is gone", () => {
    render({ default_agent: "coder" });
    const button = host.querySelector<HTMLButtonElement>('button[aria-label^="Default agent"]')!;
    expect(button.querySelector(".agent-alert")).not.toBeNull();
    act(() => button.click());
    const options = [...host.querySelectorAll('[role="option"]')];
    expect(options.map((o) => o.querySelector(".option-label")!.textContent)).toEqual(["server default", "General", "Coder"]);
    expect(options[1]!.querySelector("svg.agent-avatar")).not.toBeNull();
    expect(options[2]!.querySelector(".agent-alert")).not.toBeNull();
    expect(options[2]!.textContent).toContain("model unavailable");
  });
});

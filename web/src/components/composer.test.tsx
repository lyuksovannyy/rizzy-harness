// @vitest-environment happy-dom
// Who changed the bar's mode: the home background follows the user at once
// and the classifier only once it has settled, so each change must say
// which it was.
import { act } from "react";
import { createRoot } from "react-dom/client";
import { describe, expect, test } from "vitest";
import type { Mode } from "../lib/modes";
import { Composer } from "./Composer";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

function mount() {
  const changes: [Mode, "user" | "auto"][] = [];
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  act(() =>
    root.render(
      <Composer
        where="home"
        agents={[]}
        models={[]}
        workers={[]}
        agentId=""
        onAgentChange={() => {}}
        agenticBlocked={null}
        onSubmit={async () => {}}
        predict={(text) => (text.startsWith("http") ? "search" : "ask")}
        onModeChange={(mode, by) => changes.push([mode, by])}
      />,
    ),
  );
  const textarea = host.querySelector("textarea")!;
  const type = (value: string) =>
    act(() => {
      Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(textarea, value);
      textarea.dispatchEvent(new Event("input", { bubbles: true }));
    });
  const click = (label: string) =>
    act(() => [...host.querySelectorAll<HTMLButtonElement>(".mode-switch button")].find((b) => b.textContent === label)!.click());
  const tab = () => act(() => textarea.dispatchEvent(new KeyboardEvent("keydown", { key: "Tab", bubbles: true })));
  return { changes, type, click, tab, host, unmount: () => act(() => root.unmount()) };
}

describe("mode changes", () => {
  test("the classifier's picks are auto, clicks and Tab are the user's", () => {
    const c = mount();
    expect(c.changes).toEqual([["ask", "auto"]]);
    c.type("https://example.com");
    expect(c.changes.at(-1)).toEqual(["search", "auto"]);
    c.click("Agentic");
    expect(c.changes.at(-1)).toEqual(["agentic", "user"]);
    // Picking the mode it already has changes nothing and reports nothing.
    const n = c.changes.length;
    c.click("Agentic");
    expect(c.changes.length).toBe(n);
    c.tab();
    expect(c.changes.at(-1)).toEqual(["search", "user"]);
    c.unmount();
  });

  test("the switch has a thumb that screen readers skip", () => {
    const c = mount();
    const thumb = c.host.querySelector(".mode-switch > .mode-thumb");
    expect(thumb?.getAttribute("aria-hidden")).toBe("true");
    expect(c.host.querySelectorAll('.mode-switch [role="radio"]').length).toBe(3);
    c.unmount();
  });
});

describe("model for a mention", () => {
  const info = (id: string) => ({
    id,
    raw: id.split("::")[1]!,
    provider: id.split("::")[0]!,
    provider_id: id.split("::")[0]!,
    name: null,
    context: null,
    max_output: null,
    effort: false,
    vision: false,
    tools: false,
    default: id === "openai::gpt-4o",
    unavailable: false,
  });
  test("a leading @agent sends with that agent's default", () => {
    const host = document.createElement("div");
    document.body.append(host);
    const root = createRoot(host);
    act(() =>
      root.render(
        <Composer
          where="home"
          agents={[
            { id: "general", name: "General", description: "", builtin: true, default_model: null, main_model: "openai::gpt-4o", fallbacks: [], unavailable: [], avatar: { shape: "circle", eyes: "round", accent: "blue" } },
            { id: "coder", name: "Coder", description: "", builtin: true, default_model: "anthropic::claude-x", main_model: "anthropic::claude-x", fallbacks: [], unavailable: [], avatar: { shape: "circle", eyes: "round", accent: "blue" } },
          ]}
          models={[info("openai::gpt-4o"), info("anthropic::claude-x")]}
          workers={[]}
          agentId="general"
          onAgentChange={() => {}}
          agenticBlocked={null}
          onSubmit={async () => {}}
          predict={() => "ask"}
        />,
      ),
    );
    const textarea = host.querySelector("textarea")!;
    const select = () => host.querySelector<HTMLSelectElement>('select[aria-label="model"]')!;
    expect(select().value).toBe("openai::gpt-4o");
    act(() => {
      Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(textarea, "@coder fix this");
      textarea.dispatchEvent(new Event("input", { bubbles: true }));
    });
    expect(select().value).toBe("anthropic::claude-x");
    act(() => root.unmount());
    host.remove();
  });
});

describe("models the provider dropped", () => {
  const info = (id: string, more: { default?: boolean; unavailable?: boolean } = {}) => ({
    id,
    raw: id.split("::")[1]!,
    provider: "OpenAI",
    provider_id: "openai",
    name: null,
    context: null,
    max_output: null,
    effort: false,
    vision: false,
    tools: false,
    default: false,
    unavailable: false,
    ...more,
  });
  const mount = (models: ReturnType<typeof info>[], defaultModel?: string) => {
    const host = document.createElement("div");
    document.body.append(host);
    const root = createRoot(host);
    act(() =>
      root.render(
        <Composer
          where="home"
          agents={[]}
          models={models}
          defaultModel={defaultModel}
          workers={[]}
          agentId=""
          onAgentChange={() => {}}
          agenticBlocked={null}
          onSubmit={async () => {}}
          predict={() => "ask"}
        />,
      ),
    );
    const options = (label: string) =>
      [...host.querySelectorAll<HTMLOptionElement>(`select[aria-label="${label}"] option`)].map((o) => [o.value, o.disabled, o.textContent]);
    const value = () => host.querySelector<HTMLSelectElement>('select[aria-label="model"]')!.value;
    return { options, value, done: () => (act(() => root.unmount()), host.remove()) };
  };

  test("are not offered, and never picked by default", () => {
    const c = mount([info("openai::gpt-4", { default: true, unavailable: true }), info("openai::gpt-4o"), info("openai::gpt-4o-mini")]);
    expect(c.value()).toBe("openai::gpt-4o");
    expect(c.options("model")).toEqual([
      ["openai::gpt-4o", false, "gpt-4o"],
      ["openai::gpt-4o-mini", false, "gpt-4o-mini"],
    ]);
    expect(c.options("compare with").map((o) => o[0])).toEqual(["", "openai::gpt-4o-mini"]);
    c.done();
  });

  test("a default naming one still shows it, marked and not to be picked again", () => {
    const c = mount([info("openai::gpt-4", { unavailable: true }), info("openai::gpt-4o")], "openai::gpt-4");
    expect(c.value()).toBe("openai::gpt-4");
    expect(c.options("model")).toEqual([
      ["openai::gpt-4", true, "gpt-4 (unavailable)"],
      ["openai::gpt-4o", false, "gpt-4o"],
    ]);
    expect(c.options("compare with").map((o) => o[0])).toEqual(["", "openai::gpt-4o"]);
    c.done();
  });
});

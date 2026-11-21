// @vitest-environment happy-dom
// The composer's strip: the box holds the text, the attach button and
// send/stop; what the person picks (agent, approval mode, step limit, and on
// the home screen the model, effort, search and mode) sits in a small muted
// strip under the box, with the chat's figures at its right (SessionFigures,
// passed in by the chat). The controls are the same components with the same
// calls; the `choosing` and `readOnly` states keep what the row did.
import { act, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import { beforeAll, describe, expect, test } from "vitest";
import type { Agent, ApprovalMode, ModelInfo } from "../types";
import { Composer } from "./Composer";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

beforeAll(async () => {
  await import("./ApprovalModePick");
  await import("./StepLimitPick");
}, 30_000);

const tick = () => act(async () => new Promise((r) => setTimeout(r, 10)));
async function until(host: HTMLElement, sel: string) {
  const by = Date.now() + 4000;
  while (!host.querySelector(sel) && Date.now() < by) await tick();
}

const av = { shape: "circle" as const, eyes: "round" as const, accent: "blue" as const };
const agent = (id: string, name: string): Agent =>
  ({ id, name, description: "", builtin: true, default_model: null, main_model: "m", fallbacks: [], unavailable: [], avatar: av }) as unknown as Agent;
const AGENTS = [agent("coder", "Coder"), agent("writer", "Writer")];
const model = (id: string, def = false): ModelInfo =>
  ({ id, raw: id, provider: "P", provider_id: "p", name: null, context: null, max_output: null, effort: false, vision: false, tools: false, default: def, unavailable: false }) as unknown as ModelInfo;

interface Calls {
  agents: string[];
  modes: ApprovalMode[];
  steps: number[];
}
function mount(over: Partial<React.ComponentProps<typeof Composer>> = {}, figures?: ReactNode) {
  const calls: Calls = { agents: [], modes: [], steps: [] };
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  act(() =>
    root.render(
      <Composer
        where="session"
        agents={AGENTS}
        models={[]}
        workers={[]}
        agentId="coder"
        onAgentChange={(id) => calls.agents.push(id)}
        agenticBlocked={null}
        approval={{ value: "auto", max: "auto", host: false, onChange: async (m) => void calls.modes.push(m) }}
        steps={{ value: 1000, max: 1000, onChange: async (n) => void calls.steps.push(n) }}
        figures={figures}
        onSubmit={async () => {}}
        {...over}
      />,
    ),
  );
  return { host, calls, unmount: () => (act(() => root.unmount()), host.remove()) };
}
const names = (el: Element | null) => [...(el?.querySelectorAll<HTMLElement>("button, select, input:not([type=file]), [role=radio]") ?? [])].map((b) => b.getAttribute("aria-label") ?? b.textContent ?? "");

describe("the box and the strip (a chat)", () => {
  test("the box's row holds only attach and send; the pickers are in the strip, in order", async () => {
    const c = mount();
    await until(c.host, ".step-pick button");
    const row = c.host.querySelector(".composer .composer-row");
    expect(names(row).filter((n) => !n.startsWith("Attach") && n !== "Send")).toEqual([]);
    expect(row?.querySelector('[aria-label="Attach files"]')).not.toBeNull();
    expect(row?.querySelector('[aria-label="Send"]')).not.toBeNull();
    const strip = c.host.querySelector(".composer-strip")!;
    expect(strip.parentElement).toBe(c.host.querySelector(".composer-wrap"));
    expect(c.host.querySelector(".composer .composer-strip")).toBeNull();
    expect(names(strip.querySelector(".strip-controls"))).toEqual(["agent: Coder", "Approval mode: Auto", "1000 steps"]);
    c.unmount();
  });

  test("the pickers do what they did in the box: the same calls", async () => {
    const c = mount();
    await until(c.host, ".step-pick button");
    // The agent.
    act(() => c.host.querySelector<HTMLButtonElement>(".composer-strip .agent-pick .select-button")!.click());
    await act(async () => [...c.host.querySelectorAll<HTMLElement>('.agent-pick [role="option"]')].find((o) => o.textContent === "Writer")!.click());
    expect(c.calls.agents).toEqual(["writer"]);
    // The approval mode.
    act(() => c.host.querySelector<HTMLButtonElement>(".composer-strip .approval-pick .select-button")!.click());
    act(() => [...c.host.querySelectorAll<HTMLElement>(".approval-pick li")].find((o) => o.textContent!.startsWith("Ask"))!.click());
    expect(c.calls.modes).toEqual(["ask"]);
    // The step limit.
    act(() => c.host.querySelector<HTMLButtonElement>(".composer-strip .step-pick button")!.click());
    const input = c.host.querySelector<HTMLInputElement>('.composer-strip .step-pick input[aria-label="Step limit"]')!;
    act(() => {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, "7");
      input.dispatchEvent(new Event("input", { bubbles: true }));
    });
    await act(async () => [...c.host.querySelectorAll<HTMLButtonElement>(".step-pop button")].find((b) => b.textContent === "Set")!.click());
    expect(c.calls.steps).toEqual([7]);
    c.unmount();
  });

  test("the figures the chat passes stand at the right of the strip, after the controls", async () => {
    const c = mount({}, <span data-testid="figs">12.3k in · 1.2k out</span>);
    await until(c.host, ".step-pick button");
    const strip = c.host.querySelector(".composer-strip")!;
    const right = strip.querySelector(".strip-figures")!;
    expect(right.textContent).toBe("12.3k in · 1.2k out");
    expect(strip.querySelector(".strip-controls")!.compareDocumentPosition(right) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    c.unmount();
  });

  test("the model that answers is a small muted read-only label right after the agent picker, its full id on hover", async () => {
    const c = mount({ sessionModel: "prov::gpt-x-1" });
    await until(c.host, ".step-pick button");
    const controls = c.host.querySelector(".strip-controls")!;
    const label = controls.querySelector<HTMLElement>(".strip-model")!;
    expect(label.textContent).toBe("gpt-x-1");
    expect(label.title).toBe("Model answering in this chat: prov::gpt-x-1");
    expect(label.matches("button, select, input, [tabindex]")).toBe(false);
    const kids = [...controls.children];
    expect(kids.indexOf(label)).toBe(kids.indexOf(controls.querySelector(".agent-pick")!) + 1);
    expect(names(controls)).toEqual(["agent: Coder", "Approval mode: Auto", "1000 steps"]);
    c.unmount();
  });

  test("no model known, no label; the home screen has its model control instead", () => {
    const c = mount({});
    expect(c.host.querySelector(".strip-model")).toBeNull();
    c.unmount();
    const h = mount({ where: "home", sessionModel: "prov::gpt-x-1", approval: undefined, steps: undefined, models: [model("p::a", true), model("p::b")] });
    expect(h.host.querySelector(".strip-model")).toBeNull();
    h.unmount();
  });

  test("with no figures there is no right side", () => {
    const c = mount({ approval: undefined, steps: undefined });
    expect(c.host.querySelector(".strip-figures")).toBeNull();
    c.unmount();
  });
});

describe("while a choice has taken the field's place, and read-only", () => {
  test("choosing: the pickers stay in the strip with the note; the field and send go as before", async () => {
    const c = mount({ choosing: true, modeNote: "Applies after this question" }, <span>f</span>);
    await until(c.host, ".step-pick button");
    expect(c.host.querySelector(".composer.choosing")).not.toBeNull();
    expect(c.host.querySelector<HTMLTextAreaElement>("textarea")!.hidden).toBe(true);
    const strip = c.host.querySelector(".composer-strip")!;
    expect(strip.querySelector(".approval-pending-note")?.textContent).toBe("Applies after this question");
    expect(names(strip.querySelector(".strip-controls"))).toContain("Approval mode: Auto");
    // A change made now still goes to the server.
    act(() => strip.querySelector<HTMLButtonElement>(".approval-pick .select-button")!.click());
    act(() => [...c.host.querySelectorAll<HTMLElement>(".approval-pick li")].find((o) => o.textContent!.startsWith("Ask"))!.click());
    expect(c.calls.modes).toEqual(["ask"]);
    c.unmount();
  });

  test("the note is a line of its own above the controls, not between the pickers", async () => {
    const c = mount({ choosing: true, modeNote: "The call above still waits for your answer." });
    await until(c.host, ".step-pick button");
    const strip = c.host.querySelector(".composer-strip")!;
    const note = strip.querySelector(".approval-pending-note")!;
    expect(note.parentElement).toBe(strip);
    expect(strip.firstElementChild).toBe(note);
    expect(strip.querySelector(".strip-controls .approval-pending-note")).toBeNull();
    c.unmount();
  });

  test("the running hint is in the box, so a turn starting or ending moves nothing under it", async () => {
    const c = mount({ running: true, onStop: () => {} });
    await until(c.host, ".step-pick button");
    expect(c.host.querySelector(".composer .composer-hint")?.textContent).toMatch(/Read after the current step/);
    expect(c.host.querySelector(".composer-wrap > .composer-hint")).toBeNull();
    c.unmount();
  });

  test("read-only: no approval mode, no step limit, the agent picker stays, the figures stay", async () => {
    const c = mount({ readOnly: "Viewer" }, <span>f</span>);
    for (let i = 0; i < 3; i++) await tick();
    const strip = c.host.querySelector(".composer-strip")!;
    expect(names(strip.querySelector(".strip-controls"))).toEqual(["agent: Coder"]);
    expect(strip.querySelector(".strip-figures")?.textContent).toBe("f");
    expect(c.host.querySelector<HTMLTextAreaElement>("textarea")!.disabled).toBe(true);
    expect(c.host.querySelector<HTMLButtonElement>('[aria-label="Send"]')!.disabled).toBe(true);
    c.unmount();
  });

  test("a running chat keeps Stop in the box and the pickers in the strip", async () => {
    const c = mount({ running: true, onStop: () => {} });
    await until(c.host, ".step-pick button");
    expect(c.host.querySelector('.composer .composer-row [aria-label="Stop"]')).not.toBeNull();
    expect(c.host.querySelector('.composer-strip [aria-label="Stop"]')).toBeNull();
    expect(c.host.querySelector(".composer-strip .step-pick")).not.toBeNull();
    c.unmount();
  });
});

describe("the home screen", () => {
  const MODELS = [model("p::a", true), model("p::b")];
  const home = (over: Partial<React.ComponentProps<typeof Composer>> = {}) =>
    mount({ where: "home", models: MODELS, approval: undefined, steps: undefined, predict: () => "ask", ...over });
  const mode = (host: HTMLElement, label: string) =>
    act(() => [...host.querySelectorAll<HTMLButtonElement>(".mode-switch button")].find((b) => b.textContent === label)!.click());

  test("the model, compare and mode controls are in the strip, in their old order; the box keeps attach and send", () => {
    const c = home();
    const row = c.host.querySelector(".composer .composer-row");
    expect(names(row).filter((n) => !n.startsWith("Attach") && n !== "Send" && n !== "dictate")).toEqual([]);
    const strip = c.host.querySelector(".composer-strip .strip-controls");
    const order = names(strip);
    expect(order.slice(0, 3)).toEqual(["agent: Coder", "model", "compare with"]);
    expect(order.indexOf("model")).toBeLessThan(order.indexOf("compare with"));
    expect(order.indexOf("compare with")).toBeLessThan(order.indexOf("Ask"));
    expect(c.host.querySelector(".composer-strip .mode-switch")).not.toBeNull();
    // No session yet: no figures.
    expect(c.host.querySelector(".strip-figures")).toBeNull();
    c.unmount();
  });

  test("search mode: the kind and engine controls are in the strip, the box has no attach", () => {
    const c = home();
    mode(c.host, "Search");
    expect(c.host.querySelector(".composer-strip [aria-label='search kind']")).not.toBeNull();
    expect(c.host.querySelector(".composer-strip button[aria-label^='Search engine']")).not.toBeNull();
    expect(c.host.querySelector(".composer .composer-row [aria-label='search kind']")).toBeNull();
    expect(c.host.querySelector(".composer .attach")).toBeNull();
    c.unmount();
  });

  test("the model pick still changes the model the message goes with", async () => {
    const sent: string[] = [];
    const c = home({ onSubmit: async (s) => void sent.push(s.model) });
    const sel = c.host.querySelector<HTMLSelectElement>('.composer-strip select[aria-label="model"]')!;
    act(() => {
      sel.value = "p::b";
      sel.dispatchEvent(new Event("change", { bubbles: true }));
    });
    const ta = c.host.querySelector("textarea")!;
    act(() => {
      Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(ta, "hi");
      ta.dispatchEvent(new Event("input", { bubbles: true }));
    });
    await act(async () => c.host.querySelector<HTMLButtonElement>("button.send")!.click());
    expect(sent).toEqual(["p::b"]);
    c.unmount();
  });

  test("agentic: the node, project and approval controls are in the strip too", async () => {
    const c = home({ workers: [{ node_id: "n", hostname: "box", version: "0", connected: true, embedded: true, profile: "agentic", exec_modes: ["sandbox"], exec_gaps: [], max_approval: "auto", default_approval: "auto" }] as never });
    mode(c.host, "Agentic");
    await until(c.host, ".approval-pick .select-button");
    expect(c.host.querySelector(".composer .agentic-row")).toBeNull();
    expect(c.host.querySelector(".composer-strip .agentic-row .approval-pick")).not.toBeNull();
    c.unmount();
  });
});

// @vitest-environment happy-dom
// TD-1: the agent a new chat from the home screen gets. In Agentic mode it
// is the Agentic default (the Software Developer here), shown in the picker
// and sent; back in Ask it is the chat default again; an agent the person
// picked wins over both, in either mode.
import { act, forwardRef } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import type { Agent, NodeInfo, Prefs } from "../types";
import type { Submission } from "./Composer";

vi.mock("./Similar", () => ({ Similar: forwardRef(() => null) }));
vi.mock("../lib/features", () => ({ useFeatures: () => null }));
const { Home } = await import("./Home");

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const agent = (id: string, name: string): Agent =>
  ({
    id,
    name,
    description: "",
    builtin: true,
    default_model: null,
    main_model: "m",
    fallbacks: [],
    unavailable: [],
    avatar: { shape: "circle", eyes: "round", accent: "blue" },
  }) as Agent;
const AGENTS = [agent("general", "Assistant"), agent("coder", "Software Developer"), agent("writer", "Writer")];
const worker: NodeInfo = { node_id: "n1", hostname: "box", version: "0", connected: true, embedded: false, profile: "agentic", exec_modes: ["sandbox"], exec_gaps: [] };
const prefs = { search_url: "", default_agent: null, default_model: null, backdrop: "off", onboarded: true } as Prefs;

let host: HTMLElement;
let root: Root;
let sent: Submission[];
beforeEach(() => {
  sent = [];
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

function mount(defaults: { chat: string; agentic: string | null }) {
  act(() =>
    root.render(
      <Home
        username="alex"
        agents={AGENTS}
        agentDefaults={defaults}
        models={[]}
        workers={[worker]}
        prefs={prefs}
        agenticBlocked={null}
        workspace={null}
        onSubmit={async (s) => {
          sent.push(s);
        }}
        predict={() => null}
        onLearn={() => {}}
        onOpen={() => {}}
        seed={1}
        history={{ titles: [], banned: [] }}
      />,
    ),
  );
}
const modeButton = (label: string) => [...host.querySelectorAll<HTMLButtonElement>(".mode-switch button")].find((b) => b.textContent === label)!;
const picker = () => host.querySelector<HTMLButtonElement>('.agent-pick button[aria-label^="agent: "]')!;
function set(el: HTMLInputElement | HTMLTextAreaElement, value: string) {
  act(() => {
    Object.getOwnPropertyDescriptor(Object.getPrototypeOf(el), "value")!.set!.call(el, value);
    el.dispatchEvent(new Event("input", { bubbles: true }));
  });
}
async function send(text: string) {
  set(host.querySelector("textarea")!, text);
  await act(async () => host.querySelector<HTMLButtonElement>("button.send")!.click());
}
function pick(name: string) {
  act(() => picker().click());
  act(() => [...host.querySelectorAll<HTMLElement>('[role="option"]')].find((o) => o.textContent?.includes(name))!.click());
}

describe("the new chat's agent", () => {
  test("Agentic takes the Agentic default, shown and sent; Ask the chat default", async () => {
    mount({ chat: "general", agentic: "coder" });
    expect(picker().getAttribute("aria-label")).toBe("agent: Assistant");
    act(() => modeButton("Agentic").click());
    expect(picker().getAttribute("aria-label")).toBe("agent: Software Developer");
    set(host.querySelector<HTMLInputElement>("input.mono")!, "/home/me/proj");
    await send("fix the build");
    expect(sent.at(-1)).toMatchObject({ mode: "agentic", agentId: "coder" });
    act(() => modeButton("Ask").click());
    expect(picker().getAttribute("aria-label")).toBe("agent: Assistant");
  });

  test("an agent the person picked wins over both defaults", async () => {
    mount({ chat: "general", agentic: "coder" });
    pick("Writer");
    act(() => modeButton("Agentic").click());
    expect(picker().getAttribute("aria-label")).toBe("agent: Writer");
    set(host.querySelector<HTMLInputElement>("input.mono")!, "/home/me/proj");
    await send("draft the notes");
    expect(sent.at(-1)).toMatchObject({ mode: "agentic", agentId: "writer" });
  });

  test("without a usable Agentic default the chat default stays", async () => {
    mount({ chat: "general", agentic: null });
    act(() => modeButton("Agentic").click());
    expect(picker().getAttribute("aria-label")).toBe("agent: Assistant");
  });
});

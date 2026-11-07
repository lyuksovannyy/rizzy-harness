// @vitest-environment happy-dom
// Agent avatars: every shape, eye style and face renders; reduced motion
// keeps the faces and drops the loops; the red alert replaces an agent's
// face only when nothing can answer for it; the session decides the face.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { emptyView } from "../fold";
import type { Agent, AgentAvatar as Avatar, Item, SessionView } from "../types";
import { AgentAvatar, AgentMark, agentAlert, AVATAR_ACCENTS, EYES, SHAPES, STATES } from "./AgentAvatar";
import { DONE_MS, sessionFace, useAvatarState } from "./avatarState";
import { AgentsSettings } from "./AgentsSettings";
import { Composer } from "./Composer";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let reduce = false;
let host: HTMLElement;
let root: Root;

beforeEach(() => {
  reduce = false;
  vi.stubGlobal("matchMedia", (q: string) => ({
    matches: q.includes("reduce") && reduce,
    addEventListener: () => {},
    removeEventListener: () => {},
  }));
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
  vi.unstubAllGlobals();
  vi.useRealTimers();
});

const render = (el: React.ReactNode) => act(() => root.render(el));
const svg = () => host.querySelector<SVGSVGElement>("svg.agent-avatar")!;

const agent = (over: Partial<Agent> = {}): Agent => ({
  id: "coder",
  name: "Coder",
  description: "",
  builtin: true,
  default_model: "openai::gpt-x",
  main_model: "openai::gpt-x",
  fallbacks: [],
  unavailable: [],
  avatar: { shape: "circle", eyes: "round", accent: "blue" },
  ...over,
});

describe("AgentAvatar", () => {
  test("every shape, eye style and face renders, with two eyes", () => {
    for (const shape of SHAPES) {
      for (const eyes of EYES) {
        for (const state of STATES) {
          render(<AgentAvatar avatar={{ shape, eyes, accent: "green" }} state={state} size={24} />);
          const s = svg();
          expect([s.dataset.shape, s.dataset.eyes, s.dataset.state]).toEqual([shape, eyes, state]);
          expect(s.querySelectorAll(".av-pupil")).toHaveLength(2);
          expect(s.querySelectorAll(".av-happy")).toHaveLength(2);
          expect(s.querySelector(".av-face")!.getAttribute("d")).toBeTruthy();
          expect(s.getAttribute("width")).toBe("24");
        }
      }
    }
  });

  test("the colour is an accent token, never a literal", () => {
    for (const accent of AVATAR_ACCENTS) {
      render(<AgentAvatar avatar={{ shape: "circle", eyes: "round", accent }} />);
      expect(svg().getAttribute("style")).toContain(`--av-c: var(--accent-${accent})`);
      expect(host.innerHTML).not.toMatch(/#[0-9a-f]{3,6}\b|rgb\(/i);
    }
  });

  test("a title makes it an image; without one it is decoration", () => {
    const a: Avatar = { shape: "drop", eyes: "wide", accent: "pink" };
    render(<AgentAvatar avatar={a} title="Coder" />);
    expect(svg().getAttribute("role")).toBe("img");
    expect(svg().getAttribute("aria-label")).toBe("Coder");
    expect(svg().querySelector("title")!.textContent).toBe("Coder");
    render(<AgentAvatar avatar={a} />);
    expect(svg().getAttribute("aria-hidden")).toBe("true");
  });

  test("motion is live by default and still under reduced motion, the face kept", () => {
    const a: Avatar = { shape: "hexagon", eyes: "long", accent: "blue" };
    render(<AgentAvatar avatar={a} state="thinking" />);
    expect(svg().dataset.motion).toBe("live");
    act(() => root.unmount());
    reduce = true;
    root = createRoot(host);
    render(<AgentAvatar avatar={a} state="thinking" />);
    expect(svg().dataset.motion).toBe("still");
    expect(svg().dataset.state).toBe("thinking");
  });
});

describe("the unavailable badge", () => {
  test("whenever the main model is gone, saying whether a fallback answers", () => {
    expect(agentAlert(agent())).toBeNull();
    // Only a fallback is gone: the main model still answers.
    expect(agentAlert(agent({ fallbacks: ["a::b"], unavailable: ["a::b"] }))).toBeNull();
    // The main model is gone but a fallback answers: still red, and says so.
    expect(agentAlert(agent({ fallbacks: ["a::b"], unavailable: ["openai::gpt-x"] }))).toMatch(
      /Coder's model gpt-x is no longer available; answering with fallback b/,
    );
    expect(agentAlert(agent({ unavailable: ["openai::gpt-x"] }))).toMatch(/Coder's model gpt-x is no longer available, and no fallback/);
    expect(agentAlert(agent({ fallbacks: ["a::b"], unavailable: ["openai::gpt-x", "a::b"] }))).toMatch(/no fallback/);
  });

  test("an agent with no chain of its own, on a gone server default", () => {
    // No agent_models entry: the server lists the default it runs on.
    const bare = { default_model: null, main_model: "openai::gpt-x", fallbacks: [], unavailable: ["openai::gpt-x"] };
    expect(agentAlert(agent(bare))).toMatch(/Coder's model gpt-x is no longer available/);
    expect(agentAlert(agent({ default_model: null, main_model: "openai::gpt-y", unavailable: [] }))).toBeNull();
    // The model it runs on, when the server says, outranks the admin's chain.
    expect(agentAlert(agent({ main_model: "x::own", unavailable: ["x::own"] }))).toMatch(/model own is no longer/);
  });

  test("AgentMark shows the red alert in place of the face, with the reason", () => {
    render(<AgentMark agent={agent({ unavailable: ["openai::gpt-x"] })} />);
    const alert = host.querySelector<HTMLElement>(".agent-alert")!;
    expect(alert).not.toBeNull();
    expect(alert.title).toMatch(/no longer available/);
    expect(alert.getAttribute("aria-label")).toBe(alert.title);
    expect(host.querySelector("svg.agent-avatar")).toBeNull();
    render(<AgentMark agent={agent({ avatar: { shape: "square", eyes: "sleepy", accent: "orange" } })} state="working" />);
    expect(host.querySelector(".agent-alert")).toBeNull();
    expect(svg().dataset.shape).toBe("square");
    expect(svg().dataset.state).toBe("working");
    // The name shows beside it, so the face is not read out a second time.
    expect(svg().getAttribute("aria-hidden")).toBe("true");
    expect(svg().getAttribute("aria-label")).toBeNull();
  });
});

describe("the face follows the session", () => {
  const view = (items: Item[], over: Partial<SessionView> = {}): SessionView => ({ ...emptyView(), items, ...over });
  const tool = (done: boolean): Item => ({
    type: "tool",
    call_id: "c",
    author: null,
    tool: "shell",
    input: {},
    live_output: "",
    result: done ? { ok: true, output: "", exit_code: 0 } : null,
    approval: null,
  });
  const reply = (over: Partial<Extract<Item, { type: "assistant" }>> = {}): Item => ({
    type: "assistant",
    message_id: "m",
    text: "hi",
    done: false,
    author: null,
    variant: null,
    ...over,
  });
  const user: Item = { type: "user", turn_id: "t", text: "go" };

  test("running: text is thinking, a running tool is working, an approval is unsure", () => {
    expect(sessionFace(view([user, reply()], { running_turn: "t" }))).toBe("thinking");
    expect(sessionFace(view([user], { running_turn: "t" }))).toBe("thinking");
    expect(sessionFace(view([user, tool(false)], { running_turn: "t" }))).toBe("working");
    expect(sessionFace(view([user, tool(true)], { running_turn: "t" }))).toBe("thinking");
    expect(sessionFace(view([user, tool(false)], { running_turn: "t", pending_approvals: ["a"] }))).toBe("unsure");
  });

  test("settled: an error, a question waiting, or idle", () => {
    expect(sessionFace(undefined)).toBe("idle");
    expect(sessionFace(view([user, reply({ done: true }), { type: "turn_end", turn_id: "t", reason: "completed" }]))).toBe("idle");
    expect(sessionFace(view([user, { type: "error", message: "x" }, { type: "turn_end", turn_id: "t", reason: "error" }]))).toBe("error");
    const ask = reply({
      done: true,
      blocks: [{ type: "choices", ordinal: 0, hash: "h", id: "b", spec: { question: "?", options: [], multi: false, other: false } }],
    });
    expect(sessionFace(view([user, ask, { type: "turn_end", turn_id: "t", reason: "completed" }]))).toBe("unsure");
    expect(sessionFace(view([user, ask, { type: "turn_end", turn_id: "t", reason: "completed" }, user]))).toBe("idle");
  });

  test("a finished turn is done for a moment, then idle", () => {
    vi.useFakeTimers();
    let seen = "";
    function Probe(p: { v: SessionView }) {
      seen = useAvatarState(p.v);
      return null;
    }
    render(<Probe v={view([user], { running_turn: "t" })} />);
    expect(seen).toBe("thinking");
    render(<Probe v={view([user, reply({ done: true }), { type: "turn_end", turn_id: "t", reason: "completed" }])} />);
    expect(seen).toBe("done");
    act(() => vi.advanceTimersByTime(DONE_MS + 10));
    expect(seen).toBe("idle");
    // A failed turn shows the error at once, never the happy face.
    render(<Probe v={view([user], { running_turn: "t2" })} />);
    render(<Probe v={view([user, { type: "turn_end", turn_id: "t2", reason: "error" }])} />);
    expect(seen).toBe("error");
  });
});

describe("in the composer", () => {
  test("the agent pill and the @mention list show faces, and red for an agent that cannot answer", () => {
    const broken = agent({ unavailable: ["openai::gpt-x"] });
    render(
      <Composer
        where="home"
        agents={[agent({ id: "general", name: "General", default_model: null }), broken]}
        models={[]}
        workers={[]}
        agentId="coder"
        onAgentChange={() => {}}
        agenticBlocked={null}
        onSubmit={async () => {}}
      />,
    );
    const pick = host.querySelector<HTMLElement>(".agent-pick")!;
    expect(pick.classList.contains("bad")).toBe(true);
    expect(pick.querySelector(".select-button .agent-alert")).not.toBeNull();
    const textarea = host.querySelector("textarea")!;
    act(() => {
      Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(textarea, "@");
      textarea.setSelectionRange(1, 1);
      textarea.dispatchEvent(new Event("input", { bubbles: true }));
    });
    const rows = [...host.querySelectorAll<HTMLElement>(".mention-popup li")];
    expect(rows.map((r) => r.classList.contains("bad"))).toEqual([false, true]);
    expect(rows[0]!.querySelector("svg.agent-avatar")).not.toBeNull();
    expect(rows[1]!.querySelector(".agent-alert")).not.toBeNull();
  });
});

describe("in Settings, Agents", () => {
  test("an agent on a gone server default is red and names the model", () => {
    render(<AgentsSettings agents={[agent({ default_model: null, unavailable: ["openai::gpt-x"] })]} admin={false} onChanged={() => {}} />);
    const row = host.querySelector("li")!;
    expect(row.classList.contains("bad")).toBe(true);
    expect(row.querySelector(".agent-alert")).not.toBeNull();
    expect(row.querySelector(".agent-models")!.textContent).toBe("server default model (gpt-x, unavailable)");
  });

  test("the look editor takes focus, and gives it back when it closes", () => {
    render(<AgentsSettings agents={[agent()]} admin onChanged={() => {}} />);
    const open = [...host.querySelectorAll<HTMLButtonElement>("button")].find((b) => b.textContent === "Edit look")!;
    act(() => open.click());
    expect(document.activeElement?.getAttribute("role")).toBe("radio");
    expect(host.querySelector(".avatar-editor")!.contains(document.activeElement)).toBe(true);
    act(() => [...host.querySelectorAll<HTMLButtonElement>(".avatar-editor button")].find((b) => b.textContent === "Cancel")!.click());
    expect(document.activeElement?.textContent).toBe("Edit look");
  });
});

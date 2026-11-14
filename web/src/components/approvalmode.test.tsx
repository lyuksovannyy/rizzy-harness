// @vitest-environment happy-dom
// A chat's approval mode (T48): one control in the composer showing the
// mode, with the three choices and what each runs unasked; a choice the
// node does not allow is disabled with the reason; a new chat sends the
// person's pick only; an open chat asks the server, which the node clamps.
import { act } from "react";
import { createRoot } from "react-dom/client";
import { describe, expect, test } from "vitest";
import type { ApprovalMode, NodeInfo } from "../types";
import { Composer, type Submission } from "./Composer";
import { fold } from "../fold";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const node = (over: Partial<NodeInfo> = {}): NodeInfo => ({
  node_id: "node-1",
  hostname: "box",
  version: "0",
  connected: true,
  embedded: false,
  profile: "agentic",
  exec_modes: ["sandbox"],
  exec_gaps: [],
  max_approval: "accept_edits",
  default_approval: "ask",
  ...over,
});

const tick = () => act(async () => new Promise((r) => setTimeout(r, 10)));

async function picker(host: HTMLElement) {
  const q = () => host.querySelector<HTMLButtonElement>(".approval-pick .select-button");
  for (let i = 0; i < 50 && !q(); i++) await tick();
  return q();
}

function options(host: HTMLElement) {
  return [...host.querySelectorAll<HTMLLIElement>(".approval-pick li")].map((li) => ({
    li,
    text: li.textContent ?? "",
    disabled: li.getAttribute("aria-disabled") === "true",
  }));
}

function mountHome(workers: NodeInfo[]) {
  const sent: Submission[] = [];
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  act(() =>
    root.render(
      <Composer
        where="home"
        agents={[]}
        models={[]}
        workers={workers}
        agentId=""
        onAgentChange={() => {}}
        agenticBlocked={null}
        onSubmit={async (s) => {
          sent.push(s);
        }}
      />,
    ),
  );
  act(() => [...host.querySelectorAll<HTMLButtonElement>(".mode-switch button")].find((b) => b.textContent === "Agentic")!.click());
  const set = (el: HTMLInputElement | HTMLTextAreaElement, value: string) =>
    act(() => {
      Object.getOwnPropertyDescriptor(Object.getPrototypeOf(el), "value")!.set!.call(el, value);
      el.dispatchEvent(new Event("input", { bubbles: true }));
    });
  set(host.querySelector("textarea")!, "fix the build");
  set(host.querySelector<HTMLInputElement>("input.mono")!, "/home/me/proj");
  const send = () => act(async () => host.querySelector<HTMLButtonElement>("button.send")!.click());
  return { host, sent, send, unmount: () => act(() => root.unmount()) };
}

describe("the approval mode of a new chat", () => {
  test("shows the node's default; a choice above its ceiling is disabled with the reason", async () => {
    const c = mountHome([node()]);
    const b = (await picker(c.host))!;
    expect(b.textContent).toContain("Ask");
    act(() => b.click());
    const opts = options(c.host);
    expect(opts.map((o) => o.li.querySelector(".option-label")!.textContent)).toEqual(["Ask", "Accept edits", "Auto"]);
    const auto = opts.find((o) => o.text.startsWith("Auto"))!;
    expect(auto.disabled).toBe(true);
    expect(auto.text).toContain("max_approval_mode");
    // A disabled choice picks nothing.
    act(() => auto.li.click());
    await c.send();
    expect(c.sent[0]!.approvalMode).toBeUndefined();
    c.unmount();
  });

  test("the person's pick goes with the message, then the next chat starts at the default", async () => {
    const c = mountHome([node()]);
    const b = (await picker(c.host))!;
    act(() => b.click());
    const edits = options(c.host).find((o) => o.text.startsWith("Accept edits"))!;
    expect(edits.disabled).toBe(false);
    expect(edits.text).toContain("commands still ask");
    act(() => edits.li.click());
    expect((await picker(c.host))!.textContent).toContain("Accept edits");
    await c.send();
    expect(c.sent.map((s) => s.approvalMode)).toEqual(["accept_edits"]);
    expect((await picker(c.host))!.textContent).toContain("Ask");
    c.unmount();
  });

  test("not shown for a node that cannot change modes", async () => {
    const c = mountHome([node({ max_approval: null })]);
    for (let i = 0; i < 5; i++) await tick();
    expect(c.host.querySelector(".approval-pick")).toBeNull();
    c.unmount();
  });
});

function mountSession(approval: { value: ApprovalMode; max: ApprovalMode; host: boolean }) {
  const asked: ApprovalMode[] = [];
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  act(() =>
    root.render(
      <Composer
        where="session"
        agents={[]}
        models={[]}
        workers={[]}
        agentId=""
        onAgentChange={() => {}}
        agenticBlocked={null}
        approval={{
          ...approval,
          onChange: async (m) => {
            asked.push(m);
          },
        }}
        onSubmit={async () => {}}
      />,
    ),
  );
  return { host, asked, unmount: () => act(() => root.unmount()) };
}

describe("the approval mode of an open chat", () => {
  test("a change asks the server; a host chat never offers auto", async () => {
    const c = mountSession({ value: "accept_edits", max: "auto", host: true });
    const b = (await picker(c.host))!;
    expect(b.textContent).toContain("Accept edits");
    act(() => b.click());
    const auto = options(c.host).find((o) => o.text.startsWith("Auto"))!;
    expect(auto.disabled).toBe(true);
    expect(auto.text).toContain("host chat");
    act(() => options(c.host).find((o) => o.text.startsWith("Ask"))!.li.click());
    expect(c.asked).toEqual(["ask"]);
    c.unmount();
  });

  test("the chat's mode is the log's latest change", () => {
    const ev = (seq: number, kind: string, payload: unknown) => ({ session_id: "s", seq, ts_ms: 0, kind, payload });
    const v = fold([
      ev(1, "session_created", { title: "t", cwd: "/w", model: "m", backend: "b", node_id: "n", approval_mode: "ask", profile: "agentic" }),
      ev(2, "approval_mode_changed", { mode: "accept_edits", requested: "auto" }),
    ]);
    expect(v.meta!.approval_mode).toBe("accept_edits");
    expect(v.items).toEqual([{ type: "approval_mode_changed", mode: "accept_edits", requested: "auto" }]);
  });
});

// @vitest-environment happy-dom
// A chat's approval mode (T50): one control in the composer showing the
// mode, with the three choices and what each runs unasked; a choice the
// node does not allow is disabled with the reason; a new chat sends the
// person's pick only; an open chat asks the server, which the node clamps.
import { act } from "react";
import { createRoot } from "react-dom/client";
import { beforeAll, describe, expect, test } from "vitest";
import type { ApprovalMode, NodeInfo } from "../types";
import { Composer, type Submission } from "./Composer";
import { fold } from "../fold";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

// The picker is a lazy chunk whose first load (a module transform) can take
// over a second on a loaded runner, past the 500 ms `picker` used to poll:
// load it once here, under the hook's own limit.
beforeAll(async () => {
  await import("./ApprovalModePick");
  await import("./ProjectPicker");
}, 30_000);

const node = (over: Partial<NodeInfo> = {}): NodeInfo => ({
  node_id: "node-1",
  hostname: "box",
  version: "0",
  connected: true,
  embedded: false,
  profile: "agentic",
  exec_modes: ["sandbox"],
  exec_gaps: [],
  max_approval: "auto",
  default_approval: "auto",
  ...over,
});

const tick = () => act(async () => new Promise((r) => setTimeout(r, 10)));

/** The picker once it has rendered, polled under a deadline. */
async function picker(host: HTMLElement) {
  const q = () => host.querySelector<HTMLButtonElement>(".approval-pick .select-button");
  const by = Date.now() + 4000;
  while (!q() && Date.now() < by) await tick();
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
  // The latest folder a chat used: the Project button starts at it (X.26).
  localStorage.setItem("harness.folders", '["/home/me/proj"]');
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
  const send = async () => {
    const by = Date.now() + 4000;
    while (host.querySelector(".project-pick")?.textContent !== "proj" && Date.now() < by) await tick();
    await act(async () => host.querySelector<HTMLButtonElement>("button.send")!.click());
  };
  return { host, sent, send, unmount: () => act(() => root.unmount()) };
}

describe("the approval mode of a new chat", () => {
  test("a node whose node.toml lowers the ceiling: its default, the rest disabled with the reason", async () => {
    const c = mountHome([node({ max_approval: "ask", default_approval: "ask" })]);
    const b = (await picker(c.host))!;
    expect(b.textContent).toContain("Ask");
    act(() => b.click());
    const opts = options(c.host);
    expect(opts.map((o) => o.li.querySelector(".option-label")!.textContent)).toEqual(["Plan", "Ask", "Accept edits", "Auto"]);
    // Plan is below every ceiling: always on offer.
    expect(opts.find((o) => o.text.startsWith("Plan"))!.disabled).toBe(false);
    const auto = opts.find((o) => o.text.startsWith("Auto"))!;
    expect(auto.disabled).toBe(true);
    expect(auto.text).toContain("max_approval_mode");
    expect(opts.find((o) => o.text.startsWith("Accept edits"))!.disabled).toBe(true);
    // A disabled choice picks nothing.
    act(() => auto.li.click());
    await c.send();
    expect(c.sent[0]!.approvalMode).toBeUndefined();
    c.unmount();
  });

  test("a new chat starts in Auto; the person's pick goes with the message, then the next starts in Auto", async () => {
    const c = mountHome([node()]);
    const b = (await picker(c.host))!;
    expect(b.textContent).toContain("Auto");
    act(() => b.click());
    const edits = options(c.host).find((o) => o.text.startsWith("Accept edits"))!;
    expect(edits.disabled).toBe(false);
    expect(edits.text).toContain("commands still ask");
    act(() => edits.li.click());
    expect((await picker(c.host))!.textContent).toContain("Accept edits");
    await c.send();
    expect(c.sent.map((s) => s.approvalMode)).toEqual(["accept_edits"]);
    expect((await picker(c.host))!.textContent).toContain("Auto");
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
  test("a change asks the server; in a host chat Auto says plainly what it does", async () => {
    const c = mountSession({ value: "auto", max: "auto", host: true });
    const b = (await picker(c.host))!;
    expect(b.textContent).toContain("Auto");
    act(() => b.click());
    const auto = options(c.host).find((o) => o.text.startsWith("Auto"))!;
    expect(auto.disabled).toBe(false);
    expect(auto.text).toContain("as the node's own user, no sandbox, with no question asked");
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

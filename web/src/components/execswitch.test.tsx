// @vitest-environment happy-dom
// The owner's switch of a chat's execution mode (N.10b): the strip's
// "Commands" control is a label where there is no choice, a picker where
// the owner may switch, disabled with its reason while anything runs;
// choosing the host shell shows the warning first (with what Auto means
// there), cancelling leaves the mode, going back to the sandbox needs no
// warning; the fold takes the latest `exec_mode_changed` as the chat's mode
// and a rewind never moves it back; the new-chat label says how to turn the
// host shell on.
import { act } from "react";
import { createRoot } from "react-dom/client";
import { beforeAll, describe, expect, test } from "vitest";
import { fold } from "../fold";
import { whyNotSwitchable } from "../lib/execSwitch";
import type { ExecMode, RawEvent } from "../types";
import { ExecChoice } from "./ExecChoice";
import { ExecSwitch } from "./ExecSwitch";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

beforeAll(async () => {
  await import("./HostWarning");
}, 30_000);

const tick = () => act(async () => new Promise((r) => setTimeout(r, 10)));
async function until<T>(q: () => T | null, ms = 4000): Promise<T | null> {
  const by = Date.now() + ms;
  while (!q() && Date.now() < by) await tick();
  return q();
}

function mount(over: Partial<Parameters<typeof ExecSwitch>[0]> = {}) {
  const picked: ExecMode[] = [];
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  act(() =>
    root.render(
      <ExecSwitch
        value="sandbox"
        offered={["sandbox", "host"]}
        mayHost
        node="box"
        approval="ask"
        busy={null}
        onChange={(m) => picked.push(m)}
        {...over}
      />,
    ),
  );
  const button = () => host.querySelector<HTMLButtonElement>(".select-button");
  const open = () => act(() => button()!.click());
  const option = (word: string) => [...host.querySelectorAll<HTMLLIElement>(".select li")].find((li) => li.textContent?.startsWith(word));
  const dialog = () => until(() => document.querySelector<HTMLDialogElement>("dialog.host-warning"));
  const press = (d: Element, label: string) =>
    act(() => [...d.querySelectorAll<HTMLButtonElement>("button")].find((b) => b.textContent === label)!.click());
  return { host, picked, button, open, option, dialog, press, unmount: () => act(() => root.unmount()) };
}

describe("the strip's Commands control", () => {
  test("a label where there is no choice, saying why", () => {
    // The node offers only the sandbox.
    let c = mount({ offered: ["sandbox"] });
    expect(c.host.querySelector(".select-button")).toBeNull();
    const label = c.host.querySelector<HTMLElement>(".exec-only")!;
    expect(label.textContent).toBe("sandbox");
    expect(label.title).toContain("[execution] host = true");
    c.unmount();
    // The person's level is below host.
    c = mount({ mayHost: false });
    expect(c.host.querySelector(".select-button")).toBeNull();
    expect(c.host.querySelector<HTMLElement>(".exec-only")!.title).toContain("host level");
    c.unmount();
    // A host chat on a node that offers only the host shell.
    c = mount({ value: "host", offered: ["host"] });
    expect(c.host.querySelector(".exec-only")!.textContent).toBe("host shell");
    c.unmount();
  });

  test("the host shell opens the warning; cancel leaves the mode; confirm switches", async () => {
    const c = mount({ approval: "auto" });
    expect(c.button()!.getAttribute("aria-label")).toBe("Commands: sandbox");
    c.open();
    act(() => c.option("host shell")!.click());
    const d = (await c.dialog())!;
    const text = d.textContent!;
    expect(text).toContain("Switch this chat's commands to the host shell?");
    expect(text).toContain("as the node's own user, with nothing between them and the machine");
    // What Auto means here, and what does not carry over.
    expect(text).toContain("This chat is in Auto: from its next message every command runs in the host shell with no question asked");
    expect(text).toContain("does not carry over");
    c.press(d, "Cancel");
    expect(document.querySelector("dialog.host-warning")).toBeNull();
    expect(c.picked).toEqual([]);
    c.open();
    act(() => c.option("host shell")!.click());
    c.press((await c.dialog())!, "Switch to the host shell");
    expect(c.picked).toEqual(["host"]);
    c.unmount();
  });

  test("the warning says nothing of Auto in Ask, and back to the sandbox asks nothing", async () => {
    let c = mount({ approval: "ask" });
    c.open();
    act(() => c.option("host shell")!.click());
    const d = (await c.dialog())!;
    expect(d.textContent).not.toContain("This chat is in Auto");
    c.press(d, "Cancel");
    c.unmount();
    c = mount({ value: "host" });
    c.open();
    act(() => c.option("sandbox")!.click());
    expect(document.querySelector("dialog.host-warning")).toBeNull();
    expect(c.picked).toEqual(["sandbox"]);
    c.unmount();
  });

  test("while anything runs it is disabled and says why", () => {
    const c = mount({ busy: "Stop the agent or wait for it to finish, then switch" });
    expect(c.button()!.disabled).toBe(true);
    expect(c.host.querySelector(".exec-switch")!.getAttribute("title")).toContain("Stop the agent");
    expect(c.host.querySelector('[role="note"]')!.textContent).toContain("Stop the agent");
    c.unmount();
  });
});

describe("when the mode may be switched", () => {
  const idle = { running: false, compacting: false, waiting: false };
  test("only while nothing runs, and the reason names what to stop", () => {
    expect(whyNotSwitchable(null, idle)).toBeNull();
    expect(whyNotSwitchable(null, { ...idle, running: true })).toContain("Stop the agent");
    expect(whyNotSwitchable(null, { ...idle, waiting: true })).toContain("Answer the approval");
    expect(whyNotSwitchable(null, { ...idle, compacting: true })).toContain("summarised");
    const bg = (ended: boolean) => ({ background: { "bg-1": { call_id: "c", ends_by_ms: 0, timeout_s: 1, ended } } });
    expect(whyNotSwitchable(bg(true), idle)).toBeNull();
    expect(whyNotSwitchable(bg(false), idle)).toContain("background");
  });
});

describe("the fold", () => {
  const ev = (seq: number, kind: string, payload: Record<string, unknown>): RawEvent => ({
    session_id: "s",
    seq,
    ts_ms: seq,
    kind,
    payload,
  });
  const created = ev(1, "session_created", {
    title: "t",
    cwd: "/w",
    model: "m",
    backend: "native",
    node_id: "n",
    approval_mode: "ask",
    profile: "agentic",
    exec_mode: "sandbox",
  });
  const turn = (seq: number, id: string) => [
    ev(seq, "turn_started", { turn_id: id }),
    ev(seq + 1, "user_message", { turn_id: id, text: "go" }),
    ev(seq + 2, "turn_ended", { turn_id: id, reason: "completed" }),
  ];

  test("the latest change is the chat's mode, with a divider where it happened", () => {
    const v = fold([created, ...turn(2, "t1"), ev(5, "exec_mode_changed", { mode: "host", from: "sandbox" })]);
    expect(v.meta?.exec_mode).toBe("host");
    expect(v.items.at(-1)).toEqual({ type: "exec_mode_changed", mode: "host", from: "sandbox" });
  });

  test("a rewind over the switch hides the divider and never moves the mode back", () => {
    const v = fold([
      created,
      ...turn(2, "t1"),
      ev(5, "exec_mode_changed", { mode: "host", from: "sandbox" }),
      ...turn(6, "t2"),
      ev(9, "conversation_rewound", { message_seq: 3, turn_id: "t1" }),
    ]);
    expect(v.meta?.exec_mode).toBe("host");
  });

  test("a mode this page does not know is dropped and counted, not folded", () => {
    const v = fold([created, ev(2, "exec_mode_changed", { mode: "quantum", from: "sandbox" })]);
    expect(v.meta?.exec_mode).toBe("sandbox");
    expect(v.unknown_events).toBe(1);
  });
});

describe("the new chat's label", () => {
  test("says how to turn the host shell on when the person may use it and the node does not offer it", () => {
    const host = document.createElement("div");
    document.body.append(host);
    const root = createRoot(host);
    act(() => root.render(<ExecChoice pickable={false} hostOff node="box" offered={["sandbox"]} value={undefined} onChange={() => {}} />));
    expect(host.textContent).toContain("the host shell is off on this node");
    expect(host.textContent).toContain("host = true");
    expect(host.textContent).toContain("[execution]");
    expect(host.textContent).toContain("node.toml");
    act(() => root.render(<ExecChoice pickable={false} node="box" offered={["sandbox"]} value={undefined} onChange={() => {}} />));
    expect(host.textContent).not.toContain("the host shell is off");
    act(() => root.unmount());
  });
});

// @vitest-environment happy-dom
// The new chat's execution mode (N.10a): offered only to someone who may
// pick the host shell on a node that offers it; choosing it shows the
// warning with its risks first; cancelling keeps the sandbox and sends
// nothing; only a confirmed choice goes with the message, once.
import { act } from "react";
import { createRoot } from "react-dom/client";
import { describe, expect, test } from "vitest";
import { mayPickHost } from "../lib/execModes";
import { modeGaps } from "../lib/execWords";
import type { NodeInfo } from "../types";
import { Composer, type Submission } from "./Composer";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const node = (over: Partial<NodeInfo> = {}): NodeInfo => ({
  node_id: "node-1",
  hostname: "box",
  version: "0",
  connected: true,
  embedded: false,
  profile: "agentic",
  exec_modes: ["sandbox", "host"],
  exec_gaps: [],
  ...over,
});

function mount(workers: NodeInfo[], hostRight = { admin: true }) {
  const sent: Submission[] = [];
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const render = (workers: NodeInfo[]) =>
    root.render(
      <Composer
        where="home"
        agents={[]}
        models={[]}
        workers={workers}
        agentId=""
        onAgentChange={() => {}}
        agenticBlocked={null}
        hostRight={hostRight}
        onSubmit={async (s) => {
          sent.push(s);
        }}
      />,
    );
  act(() => render(workers));
  const rerender = (w: NodeInfo[]) => act(() => render(w));
  act(() => [...host.querySelectorAll<HTMLButtonElement>(".mode-switch button")].find((b) => b.textContent === "Agentic")!.click());
  const set = (el: HTMLInputElement | HTMLTextAreaElement | HTMLSelectElement, value: string) =>
    act(() => {
      const proto = Object.getPrototypeOf(el);
      Object.getOwnPropertyDescriptor(proto, "value")!.set!.call(el, value);
      el.dispatchEvent(new Event(el instanceof HTMLSelectElement ? "change" : "input", { bubbles: true }));
    });
  set(host.querySelector("textarea")!, "fix the build");
  set(host.querySelector<HTMLInputElement>("input.mono")!, "/home/me/proj");
  const select = () => host.querySelector<HTMLSelectElement>('select[aria-label="How this chat\'s commands run"]');
  // The choice is its own lazy chunk: wait for it (or for it not to come).
  const loaded = async () => {
    for (let i = 0; i < 50 && !select(); i++) {
      await act(async () => new Promise((r) => setTimeout(r, 10)));
    }
    return select();
  };
  const dialog = async () => {
    for (let i = 0; i < 50 && !document.querySelector("dialog.host-warning"); i++) {
      await act(async () => new Promise((r) => setTimeout(r, 10)));
    }
    return document.querySelector<HTMLDialogElement>("dialog.host-warning");
  };
  const button = (d: Element, label: string) =>
    act(() => [...d.querySelectorAll<HTMLButtonElement>("button")].find((b) => b.textContent === label)!.click());
  const send = async () => {
    await act(async () => host.querySelector<HTMLButtonElement>("button.send")!.click());
  };
  return { host, sent, set, select, loaded, dialog, button, send, rerender, unmount: () => act(() => root.unmount()) };
}

describe("the host shell for a new chat", () => {
  test("picking it shows the risks first; cancelling keeps the sandbox and sends nothing", async () => {
    const c = mount([node()]);
    const sel = (await c.loaded())!;
    expect(sel.value).toBe("sandbox");
    c.set(sel, "host");
    const d = (await c.dialog())!;
    expect(d).not.toBeNull();
    const text = d.textContent!;
    for (const risk of [
      "as the node's own user, with nothing between them and the machine",
      "read and change every file that user can, its keys and other projects included",
      "can keep running after the chat ends",
      "one web page or file the agent reads can turn all of that against you",
      "The approval card is then the only check",
      "never runs in auto",
      "what a sandbox chat wrote in this folder",
    ]) {
      expect(text).toContain(risk);
    }
    c.button(d, "Cancel");
    expect(document.querySelector("dialog.host-warning")).toBeNull();
    expect(c.select()!.value).toBe("sandbox");
    expect(c.sent).toEqual([]);
    await c.send();
    expect(c.sent.map((s) => s.execMode)).toEqual(["sandbox"]);
    c.unmount();
  });

  test("a confirmed choice goes with the message, then it is asked again", async () => {
    const c = mount([node()]);
    c.set((await c.loaded())!, "host");
    c.button((await c.dialog())!, "Use the host shell");
    expect(c.select()!.value).toBe("host");
    await c.send();
    expect(c.sent.map((s) => s.execMode)).toEqual(["host"]);
    // The next chat starts at the sandbox again.
    expect(c.select()!.value).toBe("sandbox");
    c.unmount();
  });

  test("offered to server admins only, and only where the node has a host shell", async () => {
    // Review H1: not to a plain account, workspace owner or not (every
    // account owns its Personal workspace).
    for (const [workers, right] of [
      [[node()], { admin: false }],
      [[node({ embedded: true })], { admin: false }],
      [[node({ exec_modes: ["sandbox"] })], { admin: true }],
    ] as const) {
      const c = mount([...workers], right);
      expect(await c.loaded()).toBeNull();
      c.unmount();
    }
    expect(mayPickHost(node({ embedded: true }), { admin: true })).toBe(true);
    expect(mayPickHost(node(), { admin: false })).toBe(false);
  });

  test("a confirmed choice stays with its node when the selection moves on its own", async () => {
    // Review L-j: two nodes offer the host shell; it is confirmed for the
    // first, which then drops out of the list: the bar moves to the
    // second, and the message goes in the sandbox there.
    const second = node({ node_id: "node-2", hostname: "other" });
    const c = mount([node(), second]);
    c.set((await c.loaded())!, "host");
    c.button((await c.dialog())!, "Use the host shell");
    expect(c.select()!.value).toBe("host");
    c.rerender([second]);
    expect((await c.loaded())!.value).toBe("sandbox");
    await c.send();
    expect(c.sent.map((s) => [s.nodeId, s.execMode])).toEqual([["node-2", "sandbox"]]);
    c.unmount();
  });

  test("a node with only the host shell sends nothing until it is confirmed", async () => {
    const c = mount([node({ exec_modes: ["host"] })]);
    await c.send();
    expect(c.sent).toEqual([]);
    expect(c.host.textContent).toContain("only in the host shell");
    c.unmount();
  });
});

describe("the chat header's gaps", () => {
  test("in plain words, for the chat's own mode", () => {
    expect(modeGaps("sandbox", [])).toEqual([]);
    expect(modeGaps("host", [])).toEqual(["not contained: its commands run as the node's own user, without the sandbox"]);
    expect(modeGaps("sandbox", ["gap:sandbox:netns"])[0]).toContain("UDP and DNS still go out");
    expect(modeGaps("sandbox", ["gap:host:netns"])).toEqual([]);
    expect(modeGaps("host", ["gap:host:netns"])).toHaveLength(2);
    expect(modeGaps(undefined, ["gap:host:netns"])).toEqual([]);
    // Review L-d: where nothing stops TCP, the host chat says everything goes out.
    expect(modeGaps("host", ["gap:host:net-open"])[1]).toContain("everything goes out");
    // Review L-e: the other gaps the containment line names.
    expect(modeGaps("sandbox", ["gap:sandbox:sockets"])[0]).toContain("outside the sandbox");
    expect(modeGaps("sandbox", ["gap:sandbox:proc"])[0]).toContain("/proc");
    expect(modeGaps("host", ["gap:sandbox:proc", "gap:sandbox:sockets"])).toHaveLength(1);
  });
});

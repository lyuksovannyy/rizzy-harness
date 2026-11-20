// @vitest-environment happy-dom
// X.41: a `delegate` card names the sub-agent's model and shows its report
// open under the card, through the same Markdown component as any model
// message (nothing it writes is trusted: no HTML, no remote images).
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { emptyView } from "../fold";
import { LiveCtx } from "../lib/liveCtx";
import type { Live } from "../live";
import type { Item, SessionView } from "../types";
import { childReport } from "./DelegateResult";
import { ToolCard } from "./ToolCard";

type Tool = Extract<Item, { type: "tool" }>;
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const CHILD = "0190f3a2-0000-7000-8000-00000000abcd";
let host: HTMLElement;
let root: Root;
beforeEach(() => {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  vi.stubGlobal("IntersectionObserver", undefined);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
  vi.unstubAllGlobals();
});

const quoted = (text: string) => text.split("\n").map((l) => `| ${l}\n`).join("");
const output = (report: string) =>
  `session: ${CHILD}\nSub-agent @advisor finished: done. Its own report follows, every line behind "| "; the harness did not check it.\n${quoted(report)}`;
const delegate = (out: string | null): Tool => ({
  type: "tool",
  call_id: "c1",
  author: null,
  tool: "delegate",
  input: { agent: "advisor", task: "the lead's task, folded" },
  live_output: `session: ${CHILD}\n`,
  result: out === null ? null : { ok: true, output: out, exit_code: null },
  approval: null,
});
const childView = (model: string): SessionView => ({
  ...emptyView(),
  session_id: CHILD,
  head_seq: 3,
  meta: { title: "t", cwd: "/", model, backend: "native", node_id: "n", approval_mode: "ask", profile: "agentic" } as SessionView["meta"],
});
async function show(item: Tool, model: string | null = "rizzy::claude/claude-opus-5-5") {
  let watched = false;
  const live = {
    watch: vi.fn(() => (watched = true)),
    unwatch: vi.fn(() => (watched = false)),
    view: () => (watched && model ? childView(model) : undefined),
    subscribe: () => () => {},
    version: 1,
  } as unknown as Live;
  await act(async () =>
    root.render(
      <LiveCtx.Provider value={live}>
        <ToolCard sessionId="s" item={item} />
      </LiveCtx.Provider>,
    ),
  );
  await act(async () => root.render(
    <LiveCtx.Provider value={live}>
      <ToolCard sessionId="s" item={item} />
    </LiveCtx.Provider>,
  ));
  return live;
}

describe("the delegate card", () => {
  test("names the model of the sub-agent's chat and shows the answer open, the task folded", async () => {
    await show(delegate(output("Use one transaction.\n\n- keep `hand_down`")));
    expect(host.querySelector(".child-model")!.textContent).toBe("rizzy::claude/claude-opus-5-5");
    const answer = host.querySelector(".child-answer")!;
    expect(answer.textContent).toContain("Use one transaction.");
    expect(answer.querySelector("li")!.textContent).toContain("keep");
    // The harness's own words are not part of the answer; the task stays folded.
    expect(answer.textContent).not.toContain("finished: done");
    expect(host.querySelector(".tool-head")!.getAttribute("aria-expanded")).toBe("false");
    expect(host.querySelector(".tool-body")).toBeNull();
  });

  test("a model not known yet shows nothing, and a running sub-agent has no answer block", async () => {
    await show(delegate(null), null);
    expect(host.querySelector(".child-model")!.textContent).toBe("");
    expect(host.querySelector(".child-answer")).toBeNull();
  });

  test("a long answer is cut, and Show all opens it", async () => {
    const long = Array.from({ length: 40 }, (_, i) => `line ${i} of the advice`).join("\n\n");
    await show(delegate(output(long)));
    const text = () => host.querySelector(".child-answer-text")!;
    expect(text().classList.contains("clipped")).toBe(true);
    const more = [...host.querySelectorAll("button")].find((b) => b.textContent === "Show all")!;
    await act(async () => more.click());
    expect(text().classList.contains("clipped")).toBe(false);
    expect([...host.querySelectorAll("button")].some((b) => b.textContent === "Show less")).toBe(true);
    // A short one has no button.
    await show(delegate(output("short")));
    expect([...host.querySelectorAll("button")].some((b) => b.textContent === "Show all")).toBe(false);
  });

  test("hostile markup is text, like in any message", async () => {
    await show(
      delegate(
        output(
          '<script>alert(1)</script> <img src=x onerror="alert(1)">\n\n![t](https://evil.example/p.png)\n\n[click](javascript:alert(1)) [ok](https://docs.example/x)',
        ),
      ),
    );
    const a = host.querySelector(".child-answer")!;
    expect(a.querySelector("script")).toBeNull();
    expect(a.querySelector("img")).toBeNull();
    expect(a.querySelector("[onerror]")).toBeNull();
    expect(a.textContent).toContain("<script>alert(1)</script>");
    expect(a.querySelector('a[href^="javascript"]')).toBeNull();
    expect(a.textContent).toContain("docs.example");
    expect(a.innerHTML).not.toContain('src="https://evil.example');
  });
});

describe("childReport", () => {
  test("takes the quoted report only: not the status, a cut note's words, a note or unapplied text", () => {
    const head = "session: x\nSub-agent @a finished: done. Its own report follows, every line behind \"| \"; the harness did not check it.\n";
    expect(childReport(`${head}| one\n| two\n[a harness note]\n| unapplied diff\n`).text).toBe("one\ntwo");
    const cut = childReport(`${head}| head\n(The report was cut: 5 of its 9 characters, after the first 2, are left out; the last 2 follow.)\n| tail\n`);
    expect(cut.text).toBe("head\ntail");
    expect(cut.cut).toContain("The report was cut");
    expect(childReport(`${head}(It wrote no report.)\n`).text).toBe("");
    expect(childReport("session: x\nold result without a quote\n").text).toBe("");
  });
});

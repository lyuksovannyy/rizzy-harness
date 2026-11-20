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
import { childReport, GIVE_UP_MS } from "./DelegateResult";
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
  const head = "session: x\nSub-agent @a finished: done. Its own report follows, every line behind \"| \"; the harness did not check it.\n";
  test("takes the quoted report only: not the status, a note or unapplied text", () => {
    expect(childReport(`${head}| one\n| two\n[a harness note]\n| unapplied diff\n`)).toEqual({ head: "one\ntwo", tail: null, cut: null });
    expect(childReport(`${head}(It wrote no report.)\n`).head).toBe("");
    expect(childReport("session: x\nold result without a quote\n").head).toBe("");
  });

  test("a cut report keeps its halves apart, the harness's note between them", () => {
    const r = childReport(`${head}| | a | b |\n| |---|---|\n(The report was cut: 5 of its 9 characters, after the first 2, are left out; the last 2 follow.)\n| | c | d |\n`);
    expect(r.head).toBe("| a | b |\n|---|---|");
    expect(r.tail).toBe("| c | d |");
    expect(r.cut).toContain("The report was cut");
  });

  test("leading blank lines go, an indented first line keeps its indentation", () => {
    expect(childReport(`${head}| \n| \n|     code()\n| text\n`).head).toBe("    code()\ntext");
  });
});

describe("a cut and a long report in the card", () => {
  test("the note sits between two rendered halves, written by the harness (a paragraph of its own)", async () => {
    const out = `session: ${CHILD}\nSub-agent @advisor finished: done. Its own report follows, every line behind "| "; the harness did not check it.\n| first half\n(The report was cut: 1 of its 9 characters, after the first 2, are left out; the last 2 follow.)\n| second half\n`;
    await show(delegate(out));
    const kids = [...host.querySelector(".child-answer-text")!.children].map((c) => c.className || c.tagName);
    expect(kids).toEqual(["md", "muted small child-cut", "md"]);
    expect(host.querySelector(".child-cut")!.tagName).toBe("P");
  });

  test("what is cut off is inert (no focus order into it) until Show all, which is a real button", async () => {
    const long = Array.from({ length: 40 }, (_, i) => `line ${i} [link](https://docs.example/${i})`).join("\n\n");
    await show(delegate(output(long)));
    const text = host.querySelector(".child-answer-text") as HTMLElement;
    expect(text.hasAttribute("inert")).toBe(true);
    const more = [...host.querySelectorAll("button")].find((b) => b.textContent === "Show all")!;
    expect(more.tagName).toBe("BUTTON");
    expect(more.getAttribute("aria-expanded")).toBe("false");
    await act(async () => more.click());
    expect(text.hasAttribute("inert")).toBe(false);
    expect(more.getAttribute("aria-expanded")).toBe("true");
  });
});

describe("the model's watch on the child's log", () => {
  /** A stub that counts the page's follows of each session. */
  function counting(viewOf: (watched: boolean) => SessionView | undefined) {
    let n = 0;
    const live = {
      watch: vi.fn(() => void n++),
      unwatch: vi.fn(() => void n--),
      view: () => viewOf(n > 0),
      subscribe: () => () => {},
      version: 1,
    } as unknown as Live;
    return { live, count: () => n };
  }
  const tree = (live: Live) => (
    <LiveCtx.Provider value={live}>
      <ToolCard sessionId="s" item={delegate(output("answer"))} />
    </LiveCtx.Provider>
  );

  test("returns to zero once the model is known", async () => {
    const { live, count } = counting((w) => (w ? childView("m1") : undefined));
    await act(async () => root.render(tree(live)));
    await act(async () => root.render(tree(live)));
    expect(host.querySelector(".child-model")!.textContent).toBe("m1");
    expect(count()).toBe(0);
  });

  test("unmounting before the model is known releases the watch", async () => {
    const { live, count } = counting(() => undefined);
    await act(async () => root.render(tree(live)));
    expect(count()).toBe(1);
    act(() => root.render(<div />));
    expect(count()).toBe(0);
  });

  test("a child the server does not know is let go after a while, not forever", async () => {
    vi.useFakeTimers();
    try {
      const { live, count } = counting((w) => (w ? emptyView() : undefined));
      await act(async () => root.render(tree(live)));
      expect(count()).toBe(1);
      await act(async () => void vi.advanceTimersByTime(GIVE_UP_MS + 100));
      expect(count()).toBe(0);
      expect(host.querySelector(".child-model")!.textContent).toBe("");
    } finally {
      vi.useRealTimers();
    }
  });
});

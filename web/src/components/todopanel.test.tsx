// @vitest-environment happy-dom
// The agent's task list pinned above the composer: its progress and the
// item in progress in the head, the whole list below it (collapsible),
// every item as plain text (the model wrote it).
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, describe, expect, test } from "vitest";
import type { TodoItem } from "../types";
import TodoPanel from "./TodoPanel";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLElement;
afterEach(() => host?.remove());

function mount(items: TodoItem[], running = true) {
  host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  act(() => root.render(<TodoPanel items={items} running={running} />));
  return {
    root,
    rerender: (next: TodoItem[], run = running) => act(() => root.render(<TodoPanel items={next} running={run} />)),
  };
}
const expanded = () => host.querySelector("button")!.getAttribute("aria-expanded");

const items: TodoItem[] = [
  { content: "Read the code", status: "done" },
  { content: "Write <b>the</b> tests", status: "in_progress" },
  { content: "Implement", status: "pending" },
];

describe("TodoPanel", () => {
  test("shows the progress, the item in progress and every item as text", () => {
    mount(items);
    const head = host.querySelector("button")!;
    expect(head.textContent).toContain("1 of 3 done");
    expect(head.textContent).toContain("Write <b>the</b> tests");
    expect(host.querySelector("b")).toBeNull();
    const rows = [...host.querySelectorAll("li")];
    expect(rows.map((r) => r.textContent)).toEqual(["Read the code", "Write <b>the</b> tests", "Implement"]);
    expect(rows.map((r) => r.dataset.status)).toEqual(["done", "in_progress", "pending"]);
    expect(rows[1]!.getAttribute("aria-current")).toBe("step");
  });

  test("collapses to its head and opens again", () => {
    mount(items);
    const head = host.querySelector("button")!;
    expect(head.getAttribute("aria-expanded")).toBe("true");
    act(() => head.click());
    expect(head.getAttribute("aria-expanded")).toBe("false");
    expect(host.querySelectorAll("li")).toHaveLength(0);
    expect(head.textContent).toContain("1 of 3 done");
    act(() => head.click());
    expect(host.querySelectorAll("li")).toHaveLength(3);
  });

  // TD-2: a list nobody is working on stays out of the way.
  test("starts collapsed when every item is done", () => {
    mount([
      { content: "Read the code", status: "done" },
      { content: "Ship it", status: "done" },
    ]);
    expect(expanded()).toBe("false");
    expect(host.querySelector("button")!.textContent).toContain("2 of 2 done");
  });

  test("collapses when no turn runs, even with an item in progress; opens while one runs", () => {
    const m = mount(items, false);
    expect(expanded()).toBe("false");
    expect(host.querySelector("button")!.textContent).toContain("Write <b>the</b> tests");
    m.rerender(items, true);
    expect(expanded()).toBe("true");
    m.rerender(items, false);
    expect(expanded()).toBe("false");
    // The person can still open it.
    act(() => host.querySelector("button")!.click());
    expect(host.querySelectorAll("li")).toHaveLength(3);
  });

  test("follows the latest list and is gone when it is empty", () => {
    const m = mount(items);
    m.rerender([{ content: "Ship it", status: "done" }]);
    expect(host.querySelector("button")!.textContent).toContain("1 of 1 done");
    m.rerender([]);
    expect(host.textContent).toBe("");
  });
});

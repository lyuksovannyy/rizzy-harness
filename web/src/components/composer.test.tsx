// @vitest-environment happy-dom
// Who changed the bar's mode: the home background follows the user at once
// and the classifier only once it has settled, so each change must say
// which it was.
import { act } from "react";
import { createRoot } from "react-dom/client";
import { describe, expect, test } from "vitest";
import type { Mode } from "../lib/modes";
import { Composer } from "./Composer";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

function mount() {
  const changes: [Mode, "user" | "auto"][] = [];
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  act(() =>
    root.render(
      <Composer
        where="home"
        agents={[]}
        models={[]}
        workers={[]}
        agentId=""
        onAgentChange={() => {}}
        agenticBlocked={null}
        onSubmit={async () => {}}
        predict={(text) => (text.startsWith("http") ? "search" : "ask")}
        onModeChange={(mode, by) => changes.push([mode, by])}
      />,
    ),
  );
  const textarea = host.querySelector("textarea")!;
  const type = (value: string) =>
    act(() => {
      Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(textarea, value);
      textarea.dispatchEvent(new Event("input", { bubbles: true }));
    });
  const click = (label: string) =>
    act(() => [...host.querySelectorAll<HTMLButtonElement>(".mode-switch button")].find((b) => b.textContent === label)!.click());
  const tab = () => act(() => textarea.dispatchEvent(new KeyboardEvent("keydown", { key: "Tab", bubbles: true })));
  return { changes, type, click, tab, host, unmount: () => act(() => root.unmount()) };
}

describe("mode changes", () => {
  test("the classifier's picks are auto, clicks and Tab are the user's", () => {
    const c = mount();
    expect(c.changes).toEqual([["ask", "auto"]]);
    c.type("https://example.com");
    expect(c.changes.at(-1)).toEqual(["search", "auto"]);
    c.click("Agentic");
    expect(c.changes.at(-1)).toEqual(["agentic", "user"]);
    // Picking the mode it already has changes nothing and reports nothing.
    const n = c.changes.length;
    c.click("Agentic");
    expect(c.changes.length).toBe(n);
    c.tab();
    expect(c.changes.at(-1)).toEqual(["search", "user"]);
    c.unmount();
  });

  test("the switch has a thumb that screen readers skip", () => {
    const c = mount();
    const thumb = c.host.querySelector(".mode-switch > .mode-thumb");
    expect(thumb?.getAttribute("aria-hidden")).toBe("true");
    expect(c.host.querySelectorAll('.mode-switch [role="radio"]').length).toBe(3);
    c.unmount();
  });
});

// @vitest-environment happy-dom
// A tab row that scrolls sideways (a phone) shows the chosen tab.
import { act, useState } from "react";
import { createRoot } from "react-dom/client";
import { expect, test, vi } from "vitest";
import { useRevealTab } from "./revealTab";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

test("the chosen tab is scrolled into view when it changes", () => {
  const seen: string[] = [];
  Element.prototype.scrollIntoView = vi.fn(function (this: Element) {
    seen.push(this.textContent ?? "");
  });
  const Tabs = () => {
    const [on, setOn] = useState("a");
    const ref = useRevealTab(on);
    return (
      <div ref={ref}>
        {["a", "b", "c"].map((x) => (
          <button key={x} role="tab" aria-selected={on === x} onClick={() => setOn(x)}>
            {x}
          </button>
        ))}
      </div>
    );
  };
  const host = document.createElement("div");
  const root = createRoot(host);
  act(() => root.render(<Tabs />));
  act(() => host.querySelectorAll("button")[2]!.click());
  expect(seen.at(-1)).toBe("c");
  act(() => root.unmount());
});

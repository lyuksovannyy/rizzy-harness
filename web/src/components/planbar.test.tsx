// @vitest-environment happy-dom
// Plan mode's bar (X.7): shown while the chat is in plan mode, each choice
// the person's own act: the mode it names, then "Carry out the plan." as
// their message; a choice above the node's ceiling is disabled with why.
import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, test } from "vitest";
import type { ApprovalMode } from "../types";
import PlanBar from "./PlanBar";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

function mount(max: ApprovalMode) {
  const calls: [ApprovalMode, string][] = [];
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  act(() =>
    root.render(
      <PlanBar
        max={max}
        onCarry={async (m, text) => {
          calls.push([m, text]);
        }}
      />,
    ),
  );
  const buttons = () => [...host.querySelectorAll<HTMLButtonElement>(".plan-bar-choice")];
  return { host, calls, buttons, unmount: () => act(() => root.unmount()) };
}

test("each choice switches the mode and sends the person's message", async () => {
  const c = mount("auto");
  expect(c.host.textContent).toContain("Plan mode");
  expect(c.host.textContent).toContain("nothing is changed");
  expect(c.buttons().map((b) => b.textContent)).toEqual(["Auto", "Accept edits", "Ask"]);
  // What a choice sends is visible, not only in a tooltip.
  expect(c.host.textContent).toContain("Carry out the plan.");
  await act(async () => c.buttons()[0]!.click());
  expect(c.calls).toEqual([["auto", "Carry out the plan."]]);
  c.unmount();
});

test("a choice above the node's ceiling is disabled with the reason", async () => {
  const c = mount("ask");
  const [auto, edits, ask] = c.buttons();
  expect(auto!.disabled && edits!.disabled).toBe(true);
  expect(auto!.title).toContain("max_approval_mode");
  expect(ask!.disabled).toBe(false);
  await act(async () => auto!.click());
  expect(c.calls).toEqual([]);
  c.unmount();
});

test("the review button is there only when the chat has a plan to open", () => {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  let opened = 0;
  act(() => root.render(<PlanBar max="auto" onCarry={async () => {}} />));
  expect(host.querySelector(".plan-bar-review")).toBeNull();
  act(() => root.render(<PlanBar max="auto" onCarry={async () => {}} onReview={() => void opened++} />));
  const b = host.querySelector<HTMLButtonElement>(".plan-bar-review")!;
  expect(b.textContent).toBe("Review the plan");
  act(() => b.click());
  expect(opened).toBe(1);
  act(() => root.unmount());
});

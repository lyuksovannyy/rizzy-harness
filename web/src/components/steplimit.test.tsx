// @vitest-environment happy-dom
// A chat's step limit (X.14): one compact control beside the approval
// mode, showing the chat's limit; its number field states the range and
// refuses out-of-range input with the reason before anything is sent; the
// fold keeps the limit and shows each change.
import { act } from "react";
import { createRoot } from "react-dom/client";
import { describe, expect, test } from "vitest";
import { fold } from "../fold";
import { StepLimitPick, stepError } from "./StepLimitPick";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

describe("step limit", () => {
  test("the range is checked before sending", () => {
    expect(stepError("3", 40)).toBeNull();
    expect(stepError("40", 40)).toBeNull();
    expect(stepError("0", 40)).toContain("From 1 to 40");
    expect(stepError("41", 40)).toContain("From 1 to 40");
    expect(stepError("-2", 40)).toContain("whole number");
    expect(stepError("2.5", 40)).toContain("whole number");
    expect(stepError("many", 40)).toContain("whole number");
  });

  test("the control shows the limit, states the range and sends only a valid number", async () => {
    const sent: number[] = [];
    const div = document.createElement("div");
    document.body.appendChild(div);
    const root = createRoot(div);
    await act(async () => root.render(<StepLimitPick value={1000} max={1000} onChange={(n) => void sent.push(n)} />));
    const pill = div.querySelector("button")!;
    expect(pill.textContent).toBe("1000 steps");
    await act(async () => pill.click());
    expect(div.textContent).toContain("1 to 1000");
    const input = div.querySelector("input")!;
    const set = (v: string) =>
      act(async () => {
        const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!;
        setter.call(input, v);
        input.dispatchEvent(new Event("input", { bubbles: true }));
      });
    await set("2000");
    const button = [...div.querySelectorAll("button")].find((b) => b.textContent === "Set")!;
    expect(button.disabled).toBe(true);
    expect(div.textContent).toContain("From 1 to 1000");
    await set("3");
    await act(async () => button.click());
    expect(sent).toEqual([3]);
    root.unmount();
  });

  test("the fold keeps the chat's limit and shows each change", () => {
    const v = fold([
      { session_id: "s", seq: 1, ts_ms: 0, kind: "step_limit_changed", payload: { max_steps: 3 } },
      { session_id: "s", seq: 2, ts_ms: 0, kind: "step_limit_changed", payload: { max_steps: 1000, requested: 5000 } },
    ]);
    expect(v.max_steps).toBe(1000);
    expect(v.items).toEqual([
      { type: "step_limit_changed", max_steps: 3 },
      { type: "step_limit_changed", max_steps: 1000, requested: 5000 },
    ]);
  });
});

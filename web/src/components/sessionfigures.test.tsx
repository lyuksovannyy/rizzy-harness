// @vitest-environment happy-dom
// The figures of a chat that the composer's strip shows at its right (they
// were the chat header's): tokens in and out for the whole chat, the context
// ring with its percentage, the cost when prices are known, and a quota
// placeholder that says plainly it has no number yet.
import { act } from "react";
import { createRoot } from "react-dom/client";
import { describe, expect, test } from "vitest";
import type { ModelInfo, Usage } from "../types";
import { SessionFigures } from "./SessionFigures";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const use = (input: number, output: number, cache = 0): Usage =>
  ({ input_tokens: input, output_tokens: output, context_tokens: 0, cache_read_tokens: cache, model: "p::m", by_model: { "p::m": { input_tokens: input, output_tokens: output } } }) as Usage;
const PRICED = [
  {
    id: "p::m", raw: "m", provider: "P", provider_id: "p", name: null, context: null, max_output: null,
    effort: false, vision: false, tools: false, default: false, unavailable: false,
    price: { input: 1000, output: 1000, currency: "USD" },
  },
] as unknown as ModelInfo[];

function show(over: Partial<React.ComponentProps<typeof SessionFigures>> = {}) {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const usage = over.usage ?? use(1234, 56);
  act(() =>
    root.render(<SessionFigures usage={usage} costUsage={usage} contextTokens={53760} window={128000} summaryAt={102400} model="p::m" models={[]} {...over} />),
  );
  return { host, unmount: () => (act(() => root.unmount()), host.remove()) };
}
const group = (h: HTMLElement) => h.querySelector<HTMLElement>('[role="group"]')!;

describe("a chat's figures", () => {
  test("tokens in and out for the whole chat, compact, with a hover that says what they are", () => {
    const c = show({ usage: use(1234, 56, 800) });
    const tokens = c.host.querySelector<HTMLElement>(".figure-tokens")!;
    expect(tokens.textContent).toBe("1.2k in · 56 out");
    expect(tokens.title).toBe("1,234 in · 56 out · 800 of the input read from the provider's cache");
    expect(group(c.host).getAttribute("aria-label")).toBe("This chat's usage");
    c.unmount();
  });

  test("the context ring shows its percentage; the hover says how it was worked out", () => {
    const c = show();
    const ring = c.host.querySelector<HTMLElement>(".context-meter")!;
    expect(ring.textContent).toContain("42%");
    expect(ring.querySelector("svg")?.getAttribute("aria-hidden")).toBe("true");
    expect(ring.title).toContain("42% of 128k");
    expect(ring.title).toContain("Older turns are summarised");
    c.unmount();
  });

  test("an unknown window stays in words, with the hint on how to set it", () => {
    const c = show({ window: undefined, contextTokens: 205 });
    const ring = c.host.querySelector<HTMLElement>(".context-meter")!;
    expect(ring.textContent).toContain("205 context");
    expect(ring.textContent).toContain("window unknown");
    expect(ring.querySelector("svg")).toBeNull();
    expect(ring.title).toContain("context = <tokens>");
    c.unmount();
  });

  test("the cost shows when prices are known, with the last turn's own figure, and not otherwise", () => {
    const priced = show({ models: PRICED, usage: use(1000, 200), costUsage: use(1000, 200), costTurn: use(400, 100) });
    const cost = priced.host.querySelector(".cost-meter")!;
    expect(cost.textContent).toContain("≈ $1.20");
    expect(cost.textContent).toContain("$0.50");
    priced.unmount();
    const plain = show();
    expect(plain.host.querySelector(".cost-meter")).toBeNull();
    plain.unmount();
  });

  test("a chat that spent nothing and has no context shows only the quota placeholder", () => {
    const c = show({ usage: use(0, 0), contextTokens: 0, window: undefined });
    expect(c.host.querySelector(".figure-tokens")).toBeNull();
    expect(c.host.querySelector(".context-meter")).toBeNull();
    expect(c.host.querySelector(".figure-quota")).not.toBeNull();
    c.unmount();
  });

  test("a fresh fork: nothing spent, the context it will send", () => {
    const c = show({ usage: use(0, 0), contextTokens: 205, window: undefined });
    expect(c.host.querySelector(".figure-tokens")?.textContent).toBe("0 in · 0 out");
    expect(c.host.querySelector(".context-meter")?.textContent).toContain("205");
    c.unmount();
  });
});

describe("the quota placeholder", () => {
  test("is named, says on hover and focus that it is not available yet, and is no control", () => {
    const c = show();
    const quota = c.host.querySelector<HTMLElement>(".figure-quota")!;
    expect(quota.textContent).toContain("quota");
    expect(quota.textContent).not.toMatch(/\d/);
    expect(quota.tabIndex).toBe(0);
    expect(quota.tagName).not.toBe("BUTTON");
    const tip = quota.querySelector<HTMLElement>('[role="tooltip"]')!;
    expect(tip.textContent).toBe("Your plan's quota will show here; it is not available yet.");
    expect(quota.getAttribute("aria-describedby")).toBe(tip.id);
    expect(tip.id).not.toBe("");
    c.unmount();
  });

  test("Escape on a focused placeholder dismisses its text (it stays dismissed until the pointer or focus leaves)", () => {
    const c = show();
    const quota = c.host.querySelector<HTMLElement>(".figure-quota")!;
    act(() => quota.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true })));
    expect(quota.getAttribute("data-dismissed")).toBe("true");
    c.unmount();
  });

  test("makes no request", () => {
    const calls: unknown[] = [];
    const real = globalThis.fetch;
    globalThis.fetch = ((...a: unknown[]) => (calls.push(a), real(...(a as [string])))) as typeof fetch;
    const c = show();
    c.unmount();
    globalThis.fetch = real;
    expect(calls).toEqual([]);
  });
});

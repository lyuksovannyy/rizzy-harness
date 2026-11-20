// @vitest-environment happy-dom
// A sub-agent's row says why its cost is "unpriced", with the chat header's
// words (the model and the missing price in brackets); a fully priced one
// says nothing extra.
import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, test, vi } from "vitest";
import { emptyView } from "../fold";
import type { Live } from "../live";
import { LiveCtx } from "../lib/liveCtx";
import type { ModelInfo, ModelPrice, SessionView } from "../types";
import { activityOf, activityWords, SubagentActivity } from "./SubagentActivity";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const info = (id: string, price?: ModelPrice): ModelInfo => ({
  id,
  raw: id.split("::")[1]!,
  provider: "P",
  provider_id: id.split("::")[0]!,
  name: null,
  context: null,
  max_output: null,
  effort: false,
  vision: false,
  tools: false,
  default: false,
  unavailable: false,
  ...(price ? { price: { ...price, currency: "USD" } } : {}),
});
const FULL = info("p::full", { input: 3, output: 15 });
const NO_OUT = info("p::halfpriced", { input: 3 });
const MODELS = [FULL, NO_OUT];

const CHILD = "22222222-2222-2222-2222-222222222222";
const child = (model: string): SessionView => ({
  ...emptyView(),
  session_id: CHILD,
  head_seq: 5,
  running_turn: null,
  usage: {
    input_tokens: 1000,
    output_tokens: 500,
    context_tokens: 0,
    by_model: { [model]: { input_tokens: 1000, output_tokens: 500 } },
  },
});

test("an unpriced sub-agent names the model and the missing price", () => {
  const a = activityOf(child("p::halfpriced"), MODELS);
  expect(a.cost).toContain("unpriced");
  expect(a.costHint).toContain("p::halfpriced (output)");
});

test("a fully priced sub-agent has no hint", () => {
  const a = activityOf(child("p::full"), MODELS);
  expect(a.cost).toContain("$");
  expect(a.costHint).toBeNull();
  expect(activityWords(a)).toContain("$");
});

function row(view: SessionView) {
  const live = { watch: vi.fn(), unwatch: vi.fn(), view: () => view, subscribe: () => () => {}, version: 1 } as unknown as Live;
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  act(() =>
    root.render(
      <LiveCtx.Provider value={live}>
        <SubagentActivity sessionId={CHILD} running models={MODELS} />
      </LiveCtx.Provider>,
    ),
  );
  const el = host.querySelector<HTMLElement>(".child-activity")!;
  return { el, done: () => act(() => root.unmount()) };
}

test("the row carries the hint as its hover text, only when something is unpriced", () => {
  const bad = row(child("p::halfpriced"));
  expect(bad.el.title).toContain("p::halfpriced (output)");
  bad.done();
  const good = row(child("p::full"));
  expect(good.el.title).toBe("");
  good.done();
});

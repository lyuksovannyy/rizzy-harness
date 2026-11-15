// From the independent review of PR 119, kept: the arithmetic, the form's
// parsing, formatting and drift (passed on the first version), and the
// regressions it found (failed on it): model ids that are prototype names,
// the cap applied where an event is read, a stale "last turn" after an
// interrupted turn, and the unattributed hint's wording.
import { describe, expect, it } from "vitest";
import { costMeter } from "../components/CostMeter";
import { draftOf, priceOf } from "../components/ModelPrices";
import { fold, MAX_USAGE_MODELS } from "../fold";
import type { ModelInfo, RawEvent, Usage } from "../types";
import { costText, estimate, formatCost, modelCost } from "./cost";

const info = (id: string, price?: ModelInfo["price"]): ModelInfo =>
  ({ id, name: id, context: null, max_output: null, effort: false, vision: false, tools: true, unavailable: false, price }) as unknown as ModelInfo;
const ended = (seq: number, usage?: object): RawEvent =>
  ({ session_id: "s", seq, ts_ms: 0, kind: "turn_ended", payload: { turn_id: `t${seq}`, reason: "completed", ...(usage ? { usage } : {}) } }) as RawEvent;
const p = { input: 3, cached_input: 0.3, cache_write: 3.75, output: 15, currency: "USD" };

describe("arithmetic", () => {
  it("inclusive input: uncached = input - read - write", () => {
    const c = modelCost({ input_tokens: 1_000_000, output_tokens: 0, cache_read_tokens: 600_000, cache_write_tokens: 100_000 }, info("a", p));
    expect(c).toBeCloseTo(0.3 * 3 + 0.6 * 0.3 + 0.1 * 3.75, 10);
  });
  it("a cache read above the input clamps the fresh part to 0 and still prices the read", () => {
    expect(modelCost({ input_tokens: 100, output_tokens: 0, cache_read_tokens: 1_000_000 }, info("a", p))).toBeCloseTo(0.3, 10);
  });
  it("price 0 is priced (free), missing or null is unpriced", () => {
    const free = { input: 0, output: 0, currency: "USD" };
    expect(modelCost({ input_tokens: 10, output_tokens: 10 }, info("f", free))).toBe(0);
    expect(modelCost({ input_tokens: 10, output_tokens: 10 }, info("f", { input: 0, currency: "USD" }))).toBeNull();
    expect(modelCost({ input_tokens: 10, output_tokens: 0 }, info("f", { input: null as unknown as number, currency: "USD" }))).toBeNull();
    const e = estimate({ input_tokens: 10, output_tokens: 10, context_tokens: 0, by_model: { f: { input_tokens: 10, output_tokens: 10 } } }, [info("f", free)]);
    expect(costText(e, "en-US")?.text).toBe("≈ $0.00");
  });
  it("drift over 10,000 folded turns stays far under a cent", () => {
    let u: Usage = { input_tokens: 0, output_tokens: 0, context_tokens: 0 };
    const turn = { input_tokens: 1234, output_tokens: 77, context_tokens: 0, cache_read_tokens: 333, by_model: { a: { input_tokens: 1234, output_tokens: 77, cache_read_tokens: 333 } } };
    for (let i = 0; i < 10_000; i++) u = fold([ended(1, u), ended(2, turn)]).usage;
    const exact = (10_000 * ((1234 - 333) * 3 + 333 * 0.3 + 77 * 15)) / 1e6;
    expect(Math.abs(estimate(u, [info("a", p)]).amount - exact)).toBeLessThan(1e-9);
  });
});

describe("the price form", () => {
  const d = (input: string) => ({ input, cached_input: "", cache_write: "", output: "" });
  it("empty is unpriced, '0' is free, anything but a plain decimal is refused", () => {
    expect(priceOf(draftOf(null))).toEqual({ price: null });
    expect(priceOf(d("0"))).toEqual({ price: { input: 0 } });
    expect(priceOf(d("0.25"))).toEqual({ price: { input: 0.25 } });
    expect(priceOf(d(".5"))).toEqual({ price: { input: 0.5 } });
    for (const bad of ["-1", "abc", "1e9", "Infinity", "NaN", "0x10", "1e2", "1,5"]) expect("error" in priceOf(d(bad)), bad).toBe(true);
    expect(draftOf({ input: 0 }).input).toBe("0");
  });
});

describe("formatting", () => {
  it("rounds and writes amounts", () => {
    expect(formatCost(0.004, "USD", "en-US")).toBe("< $0.01");
    expect(formatCost(0.4, "JPY", "en-US")).toBe("< ¥1");
    expect(formatCost(12.3456, "KWD", "en-US")).toMatch(/12\.346/);
    expect(formatCost(1234567.89, "USD", "en-US")).toBe("$1,234,568");
    expect(formatCost(99.996, "USD", "en-US")).toBe("$100.00");
    expect(formatCost(1, "ZZZ", "en-US")).toMatch(/ZZZ/);
  });
});

describe("model ids a node wrote", () => {
  it("prototype names are buckets, and nothing reaches a prototype", () => {
    const by = JSON.parse(
      '{"constructor":{"input_tokens":10,"output_tokens":0},"__proto__":{"input_tokens":10,"output_tokens":0},"toString":{"input_tokens":10,"output_tokens":0},"p::a":{"input_tokens":10,"output_tokens":0}}',
    );
    const usage = { input_tokens: 40, output_tokens: 0, context_tokens: 0, by_model: by };
    const v = fold([ended(1, usage), ended(2, usage)]);
    const keys = Object.keys(v.usage.by_model ?? {}).sort();
    expect(keys).toEqual(["__proto__", "constructor", "p::a", "toString"]);
    expect(Object.hasOwn(v.usage.by_model!, "__proto__")).toBe(true);
    expect(v.usage.by_model!.constructor).toEqual({ input_tokens: 20, output_tokens: 0 });
    expect(Object.getPrototypeOf(v.usage.by_model)).toBe(Object.prototype);
    expect(({} as Record<string, unknown>).input_tokens).toBeUndefined();
    expect((Object as unknown as Record<string, unknown>).input_tokens).toBeUndefined();
    expect(Object.keys(v.turn_usage!.by_model!).sort()).toEqual(keys);
    // Priced like any other id, by own key.
    expect(estimate(v.usage, [info("constructor", p)]).unpricedModels.sort()).toEqual(["__proto__", "p::a", "toString"]);
  });

  it("at the cap, ids are admitted in byte order, as the Rust fold does", () => {
    const first: Record<string, { input_tokens: number; output_tokens: number }> = {};
    for (let i = 0; i < MAX_USAGE_MODELS - 1; i++) first[`p::m${String(i).padStart(2, "0")}`] = { input_tokens: 1, output_tokens: 0 };
    const v = fold([
      ended(1, { input_tokens: 15, output_tokens: 0, context_tokens: 0, by_model: first }),
      ended(2, { input_tokens: 20, output_tokens: 0, context_tokens: 0, by_model: { "9": { input_tokens: 10, output_tokens: 0 }, "10": { input_tokens: 10, output_tokens: 0 } } }),
    ]);
    expect(Object.hasOwn(v.usage.by_model!, "10")).toBe(true);
    expect(Object.hasOwn(v.usage.by_model!, "9")).toBe(false);
  });

  it("an event's breakdown is capped where it is read", () => {
    const by: Record<string, { input_tokens: number; output_tokens: number }> = {};
    for (let i = 0; i <= MAX_USAGE_MODELS; i++) by[`q::${String(i).padStart(2, "0")}`] = { input_tokens: 1, output_tokens: 0 };
    const v = fold([ended(1, { input_tokens: 17, output_tokens: 0, context_tokens: 0, by_model: by })]);
    expect(Object.keys(v.turn_usage!.by_model!)).toHaveLength(MAX_USAGE_MODELS);
    expect(Object.hasOwn(v.turn_usage!.by_model!, "q::16")).toBe(false);
    expect(v.turn_usage!.input_tokens).toBe(17);
  });
});

describe("the last turn", () => {
  it("after an interrupted turn there is no last-turn figure", () => {
    const u = (n: number) => ({ input_tokens: n, output_tokens: 0, context_tokens: 0, by_model: { a: { input_tokens: n, output_tokens: 0 } } });
    const v = fold([ended(1, u(1_000_000)), ended(2, u(2_000_000)), ended(3)]);
    expect(v.turn_usage).toBeUndefined();
    expect(costMeter(v.usage, v.turn_usage, [info("a", p)], "en-US")?.text).toBe("≈ $9.00");
  });

  it("tokens past the cap are 'not recorded under a model'", () => {
    const by: Record<string, { input_tokens: number; output_tokens: number }> = {};
    for (let i = 0; i < 16; i++) by[`p::m${i}`] = { input_tokens: 1, output_tokens: 0 };
    const e = estimate({ input_tokens: 116, output_tokens: 0, context_tokens: 0, by_model: by }, Object.keys(by).map((k) => info(k, p)));
    const h = costText(e, "en-US")?.hint ?? "";
    expect(h).toContain("not recorded under a model");
    expect(h).not.toContain("before costs were tracked");
  });
});

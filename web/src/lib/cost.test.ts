// The cost estimate against hand calculations: uncached and cached input,
// cache writes, two models, a sub-agent and a hand-off note through the
// fold, unpriced and partly priced chats, tokens recorded under no model,
// and how amounts are rounded and written.
import { describe, expect, test } from "vitest";
import { costMeter } from "../components/CostMeter";
import { fold } from "../fold";
import type { ModelInfo, ModelPrice, ModelTokens, RawEvent, Usage } from "../types";
import { costText, estimate, formatCost, missingPrices, modelCost } from "./cost";

const info = (id: string, price?: ModelPrice, currency = "USD"): ModelInfo => ({
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
  ...(price ? { price: { ...price, currency } } : {}),
});

// Per million tokens.
const BIG = info("p::big", { input: 3, cached_input: 0.3, cache_write: 3.75, output: 15 });
const SMALL = info("p::small", { input: 0.25, output: 1.25 });
const MODELS = [BIG, SMALL, info("p::free")];

/** A usage whose every token is under `by` (totals are the sums). */
function usage(by: Record<string, ModelTokens>, extra: Partial<Usage> = {}): Usage {
  const u: Usage = { input_tokens: 0, output_tokens: 0, context_tokens: 0, by_model: by };
  for (const t of Object.values(by)) {
    u.input_tokens += t.input_tokens;
    u.output_tokens += t.output_tokens;
  }
  return { ...u, ...extra };
}

const near = (a: number, b: number) => expect(a).toBeCloseTo(b, 10);

describe("estimate", () => {
  test("uncached input and output", () => {
    // 1M in at 3 + 100k out at 15 = 3 + 1.5
    const e = estimate(usage({ "p::big": { input_tokens: 1_000_000, output_tokens: 100_000 } }), MODELS);
    near(e.amount, 4.5);
    expect(e.currency).toBe("USD");
    expect(e.unpricedTokens).toBe(0);
  });

  test("cache reads and writes are priced on their own, out of the input", () => {
    // 1M in = 200k fresh (0.6) + 600k read (0.18) + 200k written (0.75);
    // 10k out (0.15): 1.68
    const e = estimate(
      usage({
        "p::big": { input_tokens: 1_000_000, output_tokens: 10_000, cache_read_tokens: 600_000, cache_write_tokens: 200_000 },
      }),
      MODELS,
    );
    near(e.amount, 1.68);
  });

  test("two models, each at its own price", () => {
    // big 1.68 as above; small 400k in (0.1) + 40k out (0.05)
    const e = estimate(
      usage({
        "p::big": { input_tokens: 1_000_000, output_tokens: 10_000, cache_read_tokens: 600_000, cache_write_tokens: 200_000 },
        "p::small": { input_tokens: 400_000, output_tokens: 40_000 },
      }),
      MODELS,
    );
    near(e.amount, 1.83);
  });

  test("a sub-agent and a hand-off note on the small model, through the fold", () => {
    const sid = "11111111-1111-1111-1111-111111111111";
    const ev = (seq: number, kind: string, payload: object): RawEvent => ({ session_id: sid, seq, ts_ms: 0, kind, payload }) as RawEvent;
    // The turn: the lead on big (2 calls of 100k in, 1k out), its
    // sub-agent on small (200k in, 2k out); then a note on small (900 in,
    // 60 out).
    const turn = usage({
      "p::big": { input_tokens: 200_000, output_tokens: 2_000 },
      "p::small": { input_tokens: 200_000, output_tokens: 2_000 },
    });
    const note = usage({ "p::small": { input_tokens: 900, output_tokens: 60 } });
    const v = fold([
      ev(1, "turn_started", { turn_id: "t" }),
      ev(2, "turn_ended", { turn_id: "t", reason: "completed", usage: turn }),
      ev(3, "handoff_written", { through_seq: 2, text: "note", model: "p::small", usage: note }),
    ]);
    // big: 200k*3 + 2k*15 = 0.6 + 0.03; small: 200,900*0.25 + 2,060*1.25
    // = 0.050225 + 0.002575
    near(estimate(v.usage, MODELS).amount, 0.63 + 0.0528);
    // The last turn is the turn alone (the note is the chat's).
    near(estimate(v.turn_usage!, MODELS).amount, 0.63 + 0.0525);
  });

  test("a model with no price is unpriced, never free", () => {
    const e = estimate(usage({ "p::free": { input_tokens: 1000, output_tokens: 10 } }), MODELS);
    expect(e.currency).toBeNull();
    expect(e.unpricedTokens).toBe(1010);
    expect(e.unpricedModels).toEqual(["p::free"]);
    expect(costText(e)?.text).toBe("cost unpriced");
  });

  test("a model missing the price of a class it used is unpriced; the total says partly", () => {
    // small has no cache-read price: its 1k-in chat with 500 read is
    // unpriced; big's part stays (1M in at 3 = 3).
    const e = estimate(
      usage({
        "p::big": { input_tokens: 1_000_000, output_tokens: 0 },
        "p::small": { input_tokens: 1000, output_tokens: 10, cache_read_tokens: 500 },
      }),
      MODELS,
    );
    near(e.amount, 3);
    expect(e.unpricedModels).toEqual(["p::small"]);
    expect(e.unpricedTokens).toBe(1010);
    const c = costText(e, "en-US")!;
    expect(c.text).toBe("≈ $3.00 + unpriced");
    expect(c.hint).toContain("p::small");
    expect(c.hint).toContain("not the bill");
    // A class it did not use needs no price (small never wrote a cache).
    near(estimate(usage({ "p::small": { input_tokens: 4_000_000, output_tokens: 0 } }), MODELS).amount, 1);
  });

  test("tokens recorded under no model are unpriced", () => {
    // An old log: totals without a breakdown (or a model past the cap).
    const e = estimate(
      usage({ "p::big": { input_tokens: 1_000_000, output_tokens: 0 } }, { input_tokens: 1_500_000, output_tokens: 20 }),
      MODELS,
    );
    near(e.amount, 3);
    expect(e.unattributed).toBe(500_020);
    expect(costText(e, "en-US")!.text).toBe("≈ $3.00 + unpriced");
  });
});

describe("formatCost", () => {
  const f = (n: number, c: string) => formatCost(n, c, "en-US").replace(/ /g, " ");
  test("sub-cent amounts are never a rounded-down zero", () => {
    expect(f(0.004, "USD")).toBe("< $0.01");
    expect(f(0.016, "USD")).toBe("$0.02");
    expect(f(0, "USD")).toBe("$0.00");
  });
  test("cents below 100, whole units from 100", () => {
    expect(f(12.346, "USD")).toBe("$12.35");
    expect(f(99.994, "USD")).toBe("$99.99");
    expect(f(1234.5, "USD")).toBe("$1,235");
    expect(f(250_000, "EUR")).toBe("€250,000");
  });
  test("a currency without minor units, and one with three", () => {
    expect(f(0.4, "JPY")).toBe("< ¥1");
    expect(f(1234.4, "JPY")).toBe("¥1,234");
    expect(f(0.0004, "KWD")).toBe("< KWD 0.001");
    expect(f(1.2345, "KWD")).toBe("KWD 1.235");
  });
});

describe("the header", () => {
  const chat = usage({ "p::big": { input_tokens: 2_000_000, output_tokens: 0 } });
  const turn = usage({ "p::big": { input_tokens: 1_000_000, output_tokens: 0 } });
  test("the chat and its last turn", () => {
    expect(costMeter(chat, turn, MODELS, "en-US")?.text).toBe("≈ $6.00 (last turn $3.00)");
    // One turn so far: the chat's figure alone.
    expect(costMeter(chat, chat, MODELS, "en-US")?.text).toBe("≈ $6.00");
  });
  test("nothing when the server prices no model", () => {
    expect(costMeter(chat, turn, [info("p::big")], "en-US")).toBeNull();
  });

  describe("the hint names the missing price", () => {
    const hint = (u: Usage, models = MODELS) => costText(estimate(u, models), "en-US")!.hint;

    test("a model missing only the cached-input price says so", () => {
      const h = hint(usage({ "p::small": { input_tokens: 1000, output_tokens: 10, cache_read_tokens: 500 } }));
      expect(h).toContain("p::small (cached input)");
    });

    test("each missing class is named; a model with no prices says so; a priced one is not listed", () => {
      const h = hint(
        usage({
          "p::small": { input_tokens: 1000, output_tokens: 10, cache_read_tokens: 500, cache_write_tokens: 100 },
          "p::free": { input_tokens: 5, output_tokens: 5 },
          "p::big": { input_tokens: 10, output_tokens: 10 },
        }),
      );
      expect(h).toContain("p::small (cached input, cache write)");
      expect(h).toContain("p::free (no prices)");
      expect(h).not.toContain("p::big");
      // Only what the model used: unused classes are not named.
      const only = hint(usage({ "p::small": { input_tokens: 10, output_tokens: 10, cache_write_tokens: 10 } }));
      expect(only).toContain("(cache write)");
      expect(only).not.toContain("cached input");
    });

    test("input and output are named when they are the gap", () => {
      const half = info("p::half", { cached_input: 1 });
      const h = hint(usage({ "p::half": { input_tokens: 100, output_tokens: 10, cache_read_tokens: 50 } }), [BIG, half]);
      expect(h).toContain("p::half (input, output)");
    });

    test("a fully priced usage has no such hint", () => {
      const c = costText(estimate(usage({ "p::big": { input_tokens: 100, output_tokens: 10, cache_read_tokens: 50 } }), MODELS), "en-US")!;
      expect(c.hint).not.toContain("Not priced");
    });

    test("the list is capped and says how many more", () => {
      const ids = ["a", "b", "c", "d", "e"].map((x) => `p::${x}`);
      const by = Object.fromEntries(ids.map((id) => [id, { input_tokens: 10, output_tokens: 1 }]));
      const h = hint(usage(by), [BIG, ...ids.map((id) => info(id))]);
      expect(h).toContain("p::a (no prices), p::b (no prices), p::c (no prices) and 2 more");
      expect(h).not.toContain("p::d");
    });
  });

  describe("a class with no tokens never needs a price", () => {
    const priced = (price: ModelPrice) => info("p::x", price);
    const cases: [string, ModelPrice, ModelTokens, number][] = [
      [
        "input and cache read, no input price",
        { cached_input: 1, output: 2 },
        { input_tokens: 100, output_tokens: 10, cache_read_tokens: 100 },
        (100 * 1 + 10 * 2) / 1e6,
      ],
      ["no output tokens, no output price", { input: 1 }, { input_tokens: 100, output_tokens: 0 }, 100 / 1e6],
      [
        "cache write only",
        { cache_write: 4 },
        { input_tokens: 50, output_tokens: 0, cache_write_tokens: 50 },
        (50 * 4) / 1e6,
      ],
    ];
    for (const [name, price, tk, want] of cases) {
      test(name, () => {
        const m = priced(price);
        expect(modelCost(tk, m)).toBeCloseTo(want, 12);
        const e = estimate(usage({ "p::x": tk }), [m]);
        expect(Number.isFinite(e.amount)).toBe(true);
        expect(e.unpricedModels).toEqual([]);
        expect(e.amount).toBeCloseTo(want, 12);
      });
    }

    test("all 16 combinations: null exactly when a used class lacks its price, else finite", () => {
      const full: ModelPrice = { input: 1, cached_input: 2, cache_write: 3, output: 4 };
      const keys = ["input", "cached_input", "cache_write", "output"] as const;
      for (const missing of [null, ...keys]) {
        const price = { ...full };
        if (missing) delete price[missing];
        const m = priced(price);
        for (let mask = 0; mask < 16; mask++) {
          // Bit k set: class k has tokens (input counts what is left after the cache classes).
          const read = mask & 2 ? 10 : 0;
          const write = mask & 4 ? 10 : 0;
          const tk: ModelTokens = {
            input_tokens: (mask & 1 ? 10 : 0) + read + write,
            output_tokens: mask & 8 ? 10 : 0,
            cache_read_tokens: read,
            cache_write_tokens: write,
          };
          const c = modelCost(tk, m);
          const gaps = missingPrices(tk, m);
          expect(c === null, `${missing} ${mask}`).toBe(gaps.length > 0);
          if (c !== null) expect(Number.isFinite(c), `${missing} ${mask}`).toBe(true);
          expect(gaps.length > 0, `${missing} ${mask}`).toBe(missing !== null && keys.indexOf(missing) >= 0 && !!(mask & (1 << keys.indexOf(missing))));
        }
      }
    });
  });

  test("a long model id is cut in the hint", () => {
    const long = "p::" + "x".repeat(200);
    const h = costText(estimate(usage({ [long]: { input_tokens: 10, output_tokens: 1 } }), [BIG, info(long)]), "en-US")!.hint;
    expect(h).toContain("p::" + "x".repeat(56) + "… (no prices)");
    expect(h).not.toContain("x".repeat(57));
  });
});

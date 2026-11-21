// @vitest-environment happy-dom
// Settings → Server → Usage: the table per person prices with the chat
// header's own functions, sorts, says when there is nothing and when the
// server cut the scan.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import type { ModelInfo } from "../types";
import type { UsageAnswer } from "../lib/usageTotals";

vi.mock("../api", () => ({ call: vi.fn(), HttpError: class extends Error {} }));
const get = vi.fn<(from: number, to: number) => Promise<UsageAnswer>>();
vi.mock("../lib/usageTotals", async (original) => ({
  ...(await original<typeof import("../lib/usageTotals")>()),
  usageApi: { get },
}));
const { UsageSettings } = await import("./UsageSettings");

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const model = (id: string, price?: ModelInfo["price"]): ModelInfo => ({
  id,
  raw: id,
  provider: "p",
  provider_id: "p",
  name: null,
  context: null,
  max_output: null,
  effort: false,
  vision: false,
  tools: false,
  default: false,
  unavailable: false,
  ...(price ? { price } : {}),
});
const MODELS = [
  model("p::cheap", { currency: "USD", input: 1, output: 2 }),
  model("p::big", { currency: "USD", input: 10, output: 20 }),
  // No output price: unpriced.
  model("p::half", { currency: "USD", input: 1 }),
];
const row = (m: string, i: number, o: number, turns = 1) => ({
  model: m,
  input_tokens: i,
  output_tokens: o,
  cache_read_tokens: 0,
  cache_write_tokens: 0,
  turns,
});
const person = (id: string, name: string, models: ReturnType<typeof row>[]) => ({
  user_id: id,
  username: name,
  turns: models.reduce((a, m) => a + m.turns, 0),
  input_tokens: models.reduce((a, m) => a + m.input_tokens, 0),
  output_tokens: models.reduce((a, m) => a + m.output_tokens, 0),
  cache_read_tokens: 0,
  cache_write_tokens: 0,
  models,
});
const answer = (over: Partial<UsageAnswer> = {}): UsageAnswer => ({
  from: 0,
  to: 1000,
  cut: null,
  people: [
    // alex: 1M in on cheap = 1.00; bob: 1M in on big = 10.00.
    person("u1", "alex", [row("p::cheap", 1_000_000, 0)]),
    person("u2", "bob", [row("p::big", 1_000_000, 0), row("p::cheap", 5, 0)]),
  ],
  workspaces: [],
  ...over,
});

let host: HTMLElement;
let root: Root;
beforeEach(() => {
  get.mockReset();
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

const render = async () => act(async () => root.render(<UsageSettings models={MODELS} />));
const titles = () => [...host.querySelectorAll(".usage-group h4")].map((h) => h.textContent);

describe("usage totals", () => {
  test("prices each person with the header's arithmetic and sorts by cost", async () => {
    get.mockResolvedValue(answer());
    await render();
    // Cost first: bob (10.00) above alex (1.00).
    expect(titles()).toEqual(["bob", "alex"]);
    const text = host.textContent!;
    expect(text).toContain("≈ $10.00");
    expect(text).toContain("≈ $1.00");
    expect(text).toContain("from chats that still exist");
    // Rows within bob: big first.
    const first = host.querySelector(".usage-group tbody tr")!;
    expect(first.textContent).toContain("p::big");
  });

  test("each table sits in its own scroll container, so a narrow screen scrolls it, not the page", async () => {
    get.mockResolvedValue(answer());
    await render();
    const tables = host.querySelectorAll(".usage-group table");
    expect(tables).toHaveLength(2);
    for (const t of tables) expect(t.parentElement?.classList.contains("usage-scroll")).toBe(true);
  });

  test("sorts by tokens on request", async () => {
    get.mockResolvedValue(
      answer({ people: [person("u1", "alex", [row("p::cheap", 100, 0)]), person("u2", "bob", [row("p::big", 50, 0)])] }),
    );
    await render();
    // By cost bob (50 tokens on a dear model) leads; by tokens alex does.
    expect(titles()).toEqual(["bob", "alex"]);
    const sort = host.querySelector<HTMLSelectElement>(".usage-sort select")!;
    await act(async () => {
      Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, "value")!.set!.call(sort, "tokens");
      sort.dispatchEvent(new Event("change", { bubbles: true }));
    });
    expect(titles()).toEqual(["alex", "bob"]);
  });

  test("a model without an output price shows unpriced, and the total says + unpriced, with the hint", async () => {
    get.mockResolvedValue(
      answer({ people: [person("u1", "alex", [row("p::big", 1_000_000, 0), row("p::half", 1_000_000, 500_000)])] }),
    );
    await render();
    const cells = [...host.querySelectorAll("tbody td, tfoot td")].filter((c) => c.querySelector("span"));
    const texts = cells.map((c) => c.textContent);
    expect(texts).toEqual(["≈ $10.00", "cost unpriced", "≈ $10.00 + unpriced"]);
    // The hover names the class the model lacks, as the chat header does.
    expect(cells[1]!.querySelector("span")!.getAttribute("title")).toContain("p::half (output)");
    expect(host.textContent).toContain("Some models have no price");
  });

  test("nobody used anything: an empty state", async () => {
    get.mockResolvedValue(answer({ people: [] }));
    await render();
    expect(host.textContent).toContain("Nobody used any tokens in this period.");
    expect(host.querySelector("table")).toBeNull();
  });

  test("a cut scan says what share was read", async () => {
    get.mockResolvedValue(answer({ cut: { covered_percent: 37 } }));
    await render();
    expect(host.querySelector("[role=status]")!.textContent).toContain("only the most recently active 37%");
  });

  test("a preset asks for its own period", async () => {
    get.mockResolvedValue(answer());
    await render();
    const month = get.mock.calls[0]!;
    expect(month[1]).toBeGreaterThan(month[0]);
    await act(async () => [...host.querySelectorAll("button")].find((b) => b.textContent === "Last 30 days")!.click());
    const days = get.mock.calls[1]!;
    expect(days[1] - days[0]).toBe(30 * 86_400_000 + 1);
  });
});

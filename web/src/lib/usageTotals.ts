// Settings → Server → Usage: what each person used in a period, per model,
// priced where it is shown. The server sums the tokens (one number per class
// per person and model, never anything about a chat); the money is the chat
// header's own arithmetic (`estimate` in cost.ts), so a model without a price
// reads "+ unpriced" here exactly as it does there.
import { call } from "../api";
import type { ModelInfo, Usage } from "../types";
import { costText, type CostText, estimate } from "./cost";

export interface ModelRow {
  model: string;
  input_tokens: number;
  output_tokens: number;
  cache_read_tokens: number;
  cache_write_tokens: number;
  turns: number;
}

interface Totals {
  input_tokens: number;
  output_tokens: number;
  cache_read_tokens: number;
  cache_write_tokens: number;
  turns: number;
  models: ModelRow[];
}

export interface PersonRow extends Totals {
  /** Null: chats started with the API token (no account). */
  user_id: string | null;
  username: string | null;
}

export interface WorkspaceRow extends Totals {
  /** Null: Personal chats, all people's together. */
  workspace_id: string | null;
  name: string | null;
}

export interface UsageAnswer {
  from: number;
  to: number;
  /** The scan ran out of time and read only this share of the chats. */
  cut: { covered_percent: number } | null;
  people: PersonRow[];
  workspaces: WorkspaceRow[];
}

export const usageApi = {
  get: (from: number, to: number) => call<UsageAnswer>("GET", `/api/admin/usage?from=${from}&to=${to}`),
};

export type PresetId = "month" | "lastMonth" | "days30";
export interface Period {
  from: number;
  to: number;
}

/** A preset's period, in the browser's calendar. `to` is exclusive. */
export function preset(id: PresetId, now = new Date()): Period {
  const y = now.getFullYear();
  const m = now.getMonth();
  if (id === "month") return { from: new Date(y, m, 1).getTime(), to: new Date(y, m + 1, 1).getTime() };
  if (id === "lastMonth") return { from: new Date(y, m - 1, 1).getTime(), to: new Date(y, m, 1).getTime() };
  return { from: now.getTime() - 30 * 86_400_000, to: now.getTime() + 1 };
}

/** The usage `estimate` prices: the row's tokens, split by model. */
export function usageOf(t: Totals): Usage {
  const by_model: NonNullable<Usage["by_model"]> = {};
  for (const m of t.models) {
    by_model[m.model] = {
      input_tokens: m.input_tokens,
      output_tokens: m.output_tokens,
      cache_read_tokens: m.cache_read_tokens,
      cache_write_tokens: m.cache_write_tokens,
    };
  }
  return {
    input_tokens: t.input_tokens,
    output_tokens: t.output_tokens,
    cache_read_tokens: t.cache_read_tokens,
    cache_write_tokens: t.cache_write_tokens,
    context_tokens: 0,
    by_model,
  };
}

const modelUsage = (m: ModelRow): Usage => usageOf({ ...m, models: [m] });

export type SortBy = "cost" | "tokens";

export interface PricedModel {
  row: ModelRow;
  amount: number;
  cost: CostText | null;
}
export interface PricedGroup {
  /** The row it was made from (a person's or a workspace's). */
  source: PersonRow | WorkspaceRow;
  models: PricedModel[];
  amount: number;
  tokens: number;
  cost: CostText | null;
  /** Some of its tokens could not be priced (the cost says "+ unpriced"). */
  unpriced: boolean;
}

const tokensOf = (t: { input_tokens: number; output_tokens: number }) => t.input_tokens + t.output_tokens;

/** Each group and its models priced and sorted, largest first by `by` (ties
 * by name, so the order is stable). */
export function price(rows: (PersonRow | WorkspaceRow)[], models: ModelInfo[], by: SortBy, locale?: string): PricedGroup[] {
  const name = (r: PersonRow | WorkspaceRow) => ("username" in r ? r.username : r.name) ?? "";
  const key = (amount: number, tokens: number) => (by === "cost" ? amount : tokens);
  const groups = rows.map((source): PricedGroup => {
    const priced = source.models.map((row): PricedModel => {
      const e = estimate(modelUsage(row), models);
      return { row, amount: e.amount, cost: costText(e, locale) };
    });
    priced.sort((a, b) => key(b.amount, tokensOf(b.row)) - key(a.amount, tokensOf(a.row)) || a.row.model.localeCompare(b.row.model));
    const e = estimate(usageOf(source), models);
    return { source, models: priced, amount: e.amount, tokens: tokensOf(source), cost: costText(e, locale), unpriced: e.unpricedTokens > 0 };
  });
  groups.sort((a, b) => key(b.amount, b.tokens) - key(a.amount, a.tokens) || name(a.source).localeCompare(name(b.source)));
  return groups;
}

// What a chat cost, estimated where it is shown: the per-model token sums
// the log recorded (`usage.by_model`, stamped per model call by the node's
// provider router) at the prices the server's admin set per model now
// (`ModelInfo.price`). Never the bill: prices as of now, not as of the
// request, and no provider discount the harness cannot see. A model is
// priced only if every token class it used has a price; otherwise its
// tokens, and tokens recorded under no model, are counted as unpriced and
// the total says so instead of coming out too low (Oh My Pi counts
// unpriced requests separately; OpenCodex warns about coverage).
import "./i18nPages";
import { t } from "./i18n";
import type { ModelInfo, ModelTokens, Usage } from "../types";

export interface Estimate {
  /** Sum over the priced models, in `currency`. */
  amount: number;
  /** The server's currency; null when no model of this usage is priced. */
  currency: string | null;
  /** Input plus output tokens that could not be priced. */
  unpricedTokens: number;
  /** Models used with a class they have no price for (or none at all). */
  unpricedModels: string[];
  /** For each of those, what it lacks (`missingPrices`), in the same order. */
  unpricedMissing: string[][];
  /** Tokens recorded under no model (chats from before the breakdown). */
  unattributed: number;
}

const PER = 1_000_000;

/** The token classes a model used that it has no price for, in the form's
 * words; ["no prices"] for a model with none. Empty: fully priced. */
export function missingPrices(t: ModelTokens, info: ModelInfo | undefined): string[] {
  const p = info?.price;
  if (!p) return ["no prices"];
  const read = t.cache_read_tokens ?? 0;
  const write = t.cache_write_tokens ?? 0;
  const parts: [number, unknown, string][] = [
    [Math.max(0, t.input_tokens - read - write), p.input, "input"],
    [read, p.cached_input, "cached input"],
    [write, p.cache_write, "cache write"],
    [t.output_tokens, p.output, "output"],
  ];
  return parts.filter(([n, price]) => n !== 0 && (price === undefined || price === null)).map(([, , name]) => name);
}

/** One model's cost, or null when a class it used has no price (never a
 * guess: a cache read priced as input would overstate some providers tenfold). */
export function modelCost(t: ModelTokens, info: ModelInfo | undefined): number | null {
  const p = info?.price;
  if (!p || missingPrices(t, info).length > 0) return null;
  const read = t.cache_read_tokens ?? 0;
  const write = t.cache_write_tokens ?? 0;
  const fresh = Math.max(0, t.input_tokens - read - write);
  return (fresh * p.input! + read * (p.cached_input ?? 0) + write * (p.cache_write ?? 0) + t.output_tokens * p.output!) / PER;
}

export function estimate(u: Usage, models: ModelInfo[]): Estimate {
  const out: Estimate = { amount: 0, currency: null, unpricedTokens: 0, unpricedModels: [], unpricedMissing: [], unattributed: 0 };
  let input = 0;
  let output = 0;
  for (const [id, tk] of Object.entries(u.by_model ?? {})) {
    input += tk.input_tokens;
    output += tk.output_tokens;
    const info = models.find((m) => m.id === id);
    const c = modelCost(tk, info);
    if (c === null) {
      out.unpricedTokens += tk.input_tokens + tk.output_tokens;
      out.unpricedModels.push(id);
      out.unpricedMissing.push(missingPrices(tk, info));
    } else {
      out.amount += c;
      out.currency ??= info?.price?.currency ?? null;
    }
  }
  out.unattributed = Math.max(0, u.input_tokens - input) + Math.max(0, u.output_tokens - output);
  out.unpricedTokens += out.unattributed;
  return out;
}

/** Whether the server prices any model (no prices set: no cost shown). */
export function pricesSet(models: ModelInfo[]): boolean {
  return models.some((m) => m.price);
}

/** `amount` in `currency`: the currency's own minor units below 100 (none
 * for JPY, three for KWD), whole units from 100 on, and "< 0.01" for a
 * positive amount under the smallest unit, never a rounded-down zero. */
export function formatCost(amount: number, currency: string, locale?: string): string {
  let f: Intl.NumberFormat;
  try {
    f = new Intl.NumberFormat(locale, { style: "currency", currency });
  } catch {
    return `${amount.toFixed(2)} ${currency}`;
  }
  const digits = f.resolvedOptions().maximumFractionDigits ?? 2;
  const unit = 10 ** -digits;
  if (amount > 0 && amount < unit) return `< ${f.format(unit)}`;
  if (amount >= 100) {
    return new Intl.NumberFormat(locale, { style: "currency", currency, maximumFractionDigits: 0, minimumFractionDigits: 0 }).format(
      amount,
    );
  }
  return f.format(amount);
}

export interface CostText {
  /** What the meter shows ("≈ $0.42", "≈ $0.42 + unpriced", "unpriced"). */
  text: string;
  /** What hovering says. */
  hint: string;
}

/** The meter's words for a usage; null when there is nothing to say. */
export function costText(e: Estimate, locale?: string): CostText | null {
  const partly = e.unpricedTokens > 0;
  if (e.currency === null) {
    if (!partly) return null;
    return { text: t("cost.unpriced"), hint: [t("cost.noPrices"), unpricedHint(e)].filter(Boolean).join(" ") };
  }
  const money = formatCost(e.amount, e.currency, locale);
  return {
    text: partly ? t("cost.partly", { money }) : t("cost.about", { money }),
    hint: [t("cost.hint"), partly ? unpricedHint(e) : ""].filter(Boolean).join(" "),
  };
}

function unpricedHint(e: Estimate): string {
  const parts: string[] = [];
  const n = e.unpricedModels.length;
  if (n > 0) {
    const shown = e.unpricedModels.slice(0, 3).map((id, i) => `${id} (${e.unpricedMissing[i]!.join(", ")})`);
    const models = n > 3 ? t("cost.andMore", { list: shown.join(", "), n: String(n - 3) }) : shown.join(", ");
    parts.push(t("cost.unpricedModels", { models }));
  }
  if (e.unattributed > 0) parts.push(t("cost.unattributed", { n: e.unattributed.toLocaleString() }));
  return parts.join(" ");
}

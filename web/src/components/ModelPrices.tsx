// What each of a provider's models costs, set by the server's admin (no
// built-in price table: prices change, and only the admin knows the
// deal they have). Per million tokens, in the server's one currency; a
// field left empty is unknown, never free, and a chat that used that kind
// of token on that model shows as partly unpriced. Lives in the lazy
// Settings and onboarding chunks with the provider form.
import { useState } from "react";
import { pageApi } from "../apiPages";
import type { ModelPrice } from "../types";

/** The server's bound (crates/master/src/pricing.rs `MAX_PRICE`). */
export const MAX_PRICE = 10_000;

const CLASSES = [
  ["input", "Input"],
  ["cached_input", "Cached input"],
  ["cache_write", "Cache write"],
  ["output", "Output"],
] as const;

type Class = (typeof CLASSES)[number][0];

/** What the fields hold while typed. */
export type PriceDraft = Record<Class, string>;

export function draftOf(p: ModelPrice | null | undefined): PriceDraft {
  const s = (v: number | undefined) => (v === undefined || v === null ? "" : String(v));
  return { input: s(p?.input), cached_input: s(p?.cached_input), cache_write: s(p?.cache_write), output: s(p?.output) };
}

/** The price a draft says (null: none set), or what is wrong with it. */
export function priceOf(d: PriceDraft): { price: ModelPrice | null } | { error: string } {
  const price: ModelPrice = {};
  for (const [k, label] of CLASSES) {
    const raw = d[k].trim();
    if (raw === "") continue;
    // A plain decimal only: `Number` would also take "0x10" or "1e2".
    const n = /^(\d+\.?\d*|\.\d+)$/.test(raw) ? Number(raw) : Number.NaN;
    if (!Number.isFinite(n) || n < 0 || n > MAX_PRICE) return { error: `${label}: a number from 0 to ${MAX_PRICE}` };
    price[k] = n;
  }
  return { price: Object.keys(price).length ? price : null };
}

/** The chosen models' price fields. */
export function ModelPrices(props: {
  models: string[];
  drafts: Record<string, PriceDraft>;
  currency: string;
  onChange: (id: string, d: PriceDraft) => void;
}) {
  if (props.models.length === 0) return null;
  const priced = props.models.filter((id) => {
    const r = priceOf(props.drafts[id] ?? draftOf(null));
    return "price" in r && r.price;
  }).length;
  return (
    <details className="model-prices">
      <summary>
        Prices (optional) <span className="muted small">· {priced === 0 ? "none set" : `${priced} of ${props.models.length} set`}</span>
      </summary>
      <p className="muted small">
        Per million tokens, in {props.currency}, for the chat's cost estimate. An empty field is unknown, not free: a chat that used it shows as
        partly unpriced. Nothing is limited or billed by these.
      </p>
      <table className="price-table">
        <thead>
          <tr>
            <th scope="col">Model</th>
            {CLASSES.map(([k, label]) => (
              <th key={k} scope="col">
                {label}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {props.models.map((id) => {
            const d = props.drafts[id] ?? draftOf(null);
            const r = priceOf(d);
            return (
              <tr key={id}>
                <th scope="row" className="mono small ellipsis">
                  {id}
                </th>
                {CLASSES.map(([k, label]) => (
                  <td key={k}>
                    <input
                      inputMode="decimal"
                      aria-label={`${label} price of ${id}`}
                      aria-invalid={"error" in r || undefined}
                      value={d[k]}
                      placeholder="—"
                      onChange={(e) => props.onChange(id, { ...d, [k]: e.target.value })}
                    />
                  </td>
                ))}
                {"error" in r && <td className="error-text small">{r.error}</td>}
              </tr>
            );
          })}
        </tbody>
      </table>
    </details>
  );
}

/** What the server takes (`pricing::clean_currency`, after upper-casing). */
export const CURRENCY_RE = /^[A-Z]{3}$/;

/** The server's one currency for every price. */
export function CurrencyField(props: { currency: string; onSaved: (c: string) => void }) {
  const [value, setValue] = useState(props.currency);
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const save = async () => {
    setBusy(true);
    setErr(null);
    try {
      const r = await pageApi.savePricing(value);
      setValue(r.currency);
      props.onSaved(r.currency);
    } catch (x) {
      setErr(x instanceof Error ? x.message : String(x));
    } finally {
      setBusy(false);
    }
  };
  return (
    <div className="stack">
      <div className="row">
        <label className="grow">
          Currency of model prices <span className="muted small">· ISO code, one for the whole server; changing it converts no price</span>
          <input className="mono" value={value} maxLength={3} onChange={(e) => setValue(e.target.value.toUpperCase())} />
        </label>
        <button type="button" disabled={busy || value === props.currency || !CURRENCY_RE.test(value)} onClick={() => void save()}>
          Save
        </button>
      </div>
      {err && <div className="error-text">{err}</div>}
    </div>
  );
}

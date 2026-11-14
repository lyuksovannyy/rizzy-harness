// The chat header's context meter: the last request's size against the
// window of the model the turn started on, and where the next summary is
// written. Both limits come from the turn's own `turn_ended.usage`, so the
// meter shows what the node went by, not a guess made here.
import "../lib/i18nPages";
import { t } from "../lib/i18n";
import { shownName } from "../md/hidden";

/** 1234 -> "1.2k", 128000 -> "128k". */
export function compactTokens(n: number): string {
  return n >= 1000 ? `${(n / 1000).toFixed(n >= 10000 ? 0 : 1)}k` : String(n);
}

const pct = (part: number, whole: number) => String(Math.round((100 * part) / whole));

export interface Meter {
  /** What the header shows. */
  text: string;
  /** What hovering says (how it was worked out, or how to set the window). */
  hint: string | null;
  /** The window is not known: the meter says so, visibly. */
  unknown: boolean;
}

/** Pure, for tests. `used`: the last request's input tokens. */
export function meter(used: number, window: number | undefined, at: number | undefined, model: string): Meter {
  if (window && window > 0) {
    const summary =
      at === undefined
        ? null
        : at === 0
          ? t("conv.summaryNever")
          : at <= window
            ? t("conv.summaryAt", { pct: pct(at, window) })
            : t("conv.summaryAtTokens", { n: compactTokens(at) });
    const vars = { used: used.toLocaleString(), window: window.toLocaleString(), at: (at ?? 0).toLocaleString() };
    return {
      text: [t("conv.ofWindow", { pct: pct(used, window), window: compactTokens(window) }), summary].filter(Boolean).join(" · "),
      hint: at === undefined ? null : t(at === 0 ? "conv.meterNever" : "conv.meterHint", vars),
      unknown: false,
    };
  }
  const base = `${compactTokens(used)} ${t("conv.context")}`;
  // A backend that reports no limits (not this harness's loop): the bare size.
  if (at === undefined) return { text: base, hint: null, unknown: false };
  return {
    text: base,
    hint: t("conv.windowHint", { model: shownName(model) || "this model", at: at.toLocaleString() }),
    unknown: true,
  };
}

export function ContextMeter(props: { used: number; window?: number; summaryAt?: number; model: string }) {
  const m = meter(props.used, props.window, props.summaryAt, props.model);
  return (
    <span className="context-meter" title={m.hint ?? undefined}>
      {m.text}
      {m.unknown && (
        <>
          {" · "}
          <span className="window-unknown">{t("conv.windowUnknown")}</span>
        </>
      )}
    </span>
  );
}

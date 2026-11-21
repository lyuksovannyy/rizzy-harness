// The chat's figures, at the right of the composer's strip (they were the
// chat header's): the tokens in and out of the whole chat, the cost when the
// server prices the model, the context ring, and a placeholder for the
// plan's quota. Each has its own hover text. Computed per render from the
// numbers it is given: the chat's view is mutated in place, so nothing here
// keeps a copy of it.
import { useId, useState } from "react";
import "../lib/i18nPages";
import { t } from "../lib/i18n";
import type { ModelInfo, Usage } from "../types";
import { compactTokens, ContextMeter } from "./ContextMeter";
import { CostMeter } from "./CostMeter";

export function SessionFigures(props: {
  /** The chat's tokens (the chat header's figures: `Figures.usage`). */
  usage: Usage;
  /** What the cost estimate is made of, and the running turn's own part. */
  costUsage: Usage;
  costTurn?: Usage;
  /** The last request's input tokens, the model's window and the summary point. */
  contextTokens: number;
  window?: number;
  summaryAt?: number;
  model: string;
  models: ModelInfo[];
}) {
  const { usage } = props;
  const spent = usage.input_tokens + usage.output_tokens > 0;
  // A fresh fork has spent nothing but will send the copied context: both show.
  const shown = spent || props.contextTokens > 0;
  const cache = usage.cache_read_tokens
    ? ` · ${t("figures.cache", { n: usage.cache_read_tokens.toLocaleString() })}`
    : "";
  return (
    <div className="session-figures meter" role="group" aria-label={t("figures.label")}>
      {shown && (
        <>
          <span className="figure figure-tokens" title={`${usage.input_tokens.toLocaleString()} ${t("figures.in")} · ${usage.output_tokens.toLocaleString()} ${t("figures.out")}${cache}`}>
            {compactTokens(usage.input_tokens)} {t("figures.in")} · {compactTokens(usage.output_tokens)} {t("figures.out")}
          </span>
          <CostMeter usage={props.costUsage} turn={props.costTurn} models={props.models} />
          <ContextMeter ring used={props.contextTokens} window={props.window} summaryAt={props.summaryAt} model={props.model} />
        </>
      )}
      <Quota />
    </div>
  );
}

/** A placeholder: no number, no request. A ring drawn dashed, the word, and
 * the reason on hover and on keyboard focus (a title shows on neither focus
 * nor touch); Escape hides it while the pointer or focus stays. */
function Quota() {
  const id = useId();
  const [dismissed, setDismissed] = useState(false);
  return (
    <span
      className="figure figure-quota"
      tabIndex={0}
      aria-describedby={id}
      data-dismissed={dismissed || undefined}
      onKeyDown={(e) => e.key === "Escape" && setDismissed(true)}
      onPointerLeave={() => setDismissed(false)}
      onBlur={() => setDismissed(false)}
    >
      <svg className="ring ring-empty" width="14" height="14" viewBox="0 0 14 14" aria-hidden="true">
        <circle className="ring-track" cx="7" cy="7" r="5.5" />
      </svg>
      {t("figures.quota")}
      <span id={id} role="tooltip" className="figure-tip">
        {t("figures.quotaHint")}
      </span>
    </span>
  );
}

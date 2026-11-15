// The chat header's cost: the chat's and the last turn's estimate at the
// prices the server's admin set, next to the token counts. Shows nothing
// when the server prices no model (the tokens alone, as before).
import { costText, estimate, formatCost, pricesSet } from "../lib/cost";
import { t } from "../lib/i18n";
import type { ModelInfo, Usage } from "../types";

/** Pure, for tests: what the header shows and what hovering says. */
export function costMeter(usage: Usage, turn: Usage | undefined, models: ModelInfo[], locale?: string) {
  if (!pricesSet(models)) return null;
  const chat = costText(estimate(usage, models), locale);
  if (!chat) return null;
  const last = turn ? estimate(turn, models) : null;
  // The last turn's figure only when it is a figure and not the whole chat.
  const lastMoney =
    last && last.currency !== null && turn !== undefined && turn.input_tokens + turn.output_tokens < usage.input_tokens + usage.output_tokens
      ? formatCost(last.amount, last.currency, locale) + (last.unpricedTokens > 0 ? " +" : "")
      : null;
  return {
    text: lastMoney ? t("cost.withLast", { chat: chat.text, turn: lastMoney }) : chat.text,
    hint: chat.hint,
  };
}

export function CostMeter(props: { usage: Usage; turn?: Usage; models: ModelInfo[] }) {
  const m = costMeter(props.usage, props.turn, props.models);
  if (!m) return null;
  return (
    <>
      <span className="cost-meter" title={m.hint}>
        {m.text}
      </span>
      {" · "}
    </>
  );
}

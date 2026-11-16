// The line under the chat that says what the turn is doing, where the answer
// will appear: "waiting for the model" from the click on Send (the message
// itself is marked "Sending…" until the log has it) until the first word,
// with a quiet count of the seconds once the wait is long enough to wonder
// about; then what the agent does. The wait
// and the answer are told apart. Screen readers hear the wait once, from a
// region that is always there; the count is not read out every second.
import { type ReactNode, useEffect, useState } from "react";
import { t } from "../lib/i18n";
import "../lib/i18nPages";
import type { Item, SessionView } from "../types";

/** Seconds of waiting before the count shows. */
export const COUNT_FROM_S = 5;

export type TurnState = "sending" | "waiting" | "answering" | "working" | "asking" | null;

/** What the chat's turn does now: `sending` while this tab's message is not
 * in the log yet and no turn runs; `waiting` while a model call has shown
 * nothing (after the message, a tool's result or a retry). */
export function turnState(view: SessionView | null | undefined, sending: boolean): TurnState {
  if (!view?.running_turn) return sending ? "sending" : null;
  if (view.pending_approvals.length) return "asking";
  const items: Item[] = view.items;
  // A turn's end before its next message (`turn_started` comes first) is a
  // wait too, not the last turn's answer.
  const last = items.findLast((it) => ["assistant", "tool", "user", "callback", "retry", "turn_end"].includes(it.type));
  if (last?.type === "assistant") return "answering";
  if (last?.type === "tool") return last.result ? "waiting" : "working";
  return "waiting";
}

/** Whole seconds since `since` (ms), never below 0. */
export const waited = (since: number, now: number) => Math.max(0, Math.floor((now - since) / 1000));

export function TurnStatus(props: {
  state: TurnState;
  /** When the wait began (the last event's time, or the send's). */
  since: number;
  name: string;
  mark?: ReactNode;
}) {
  const { state, since } = props;
  const [now, setNow] = useState(Date.now);
  const counting = state === "waiting" || state === "sending";
  useEffect(() => {
    if (!counting) return;
    setNow(Date.now());
    const id = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(id);
  }, [counting, since]);
  const s = waited(since, now);
  const words = counting
    ? t("conv.waiting", { name: props.name })
    : state === "asking"
      ? `${props.name} is waiting for you`
      : state === "working"
        ? `${props.name} is working…`
        : `${props.name} is answering…`;
  return (
    <>
      {/* Always there, so a screen reader hears the wait once when it
          begins (a live region inserted with its text is often missed); the
          answer and the tools are not announced, as before. */}
      <span className="sr-only" role="status">
        {counting ? words : ""}
      </span>
      {state && (
        <div className={`thinking turn-${state}`} data-testid="turn-status" aria-hidden="true">
          {props.mark}
          <span>{words}</span>
          {counting && s >= COUNT_FROM_S && <span className="waited">{t("conv.waited", { s: String(s) })}</span>}
        </div>
      )}
    </>
  );
}

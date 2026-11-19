// The line under the chat that says what the turn is doing, where the answer
// will appear: "waiting for the model" from the click on Send (the message
// itself is marked "Sending…" until the log has it) until the first word,
// with a quiet count of the seconds once the wait is long enough to wonder
// about; then what the agent does. The wait and the answer are told apart.
// Screen readers hear the wait once per turn, from a region that is always
// there; the count is never read out.
import { type ReactNode, useEffect, useState } from "react";
import type { ConnState } from "../live";
import { t } from "../lib/i18n";
import "../lib/i18nPages";
import type { Item, SessionView } from "../types";

/** Seconds of waiting before the count shows. */
export const COUNT_FROM_S = 5;

export type TurnState = "sending" | "waiting" | "answering" | "working" | "asking" | null;

/** What the chat's turn does now: `sending` while this tab's message is not
 * in the log yet and no turn runs; `waiting` while a model call has shown
 * nothing (after the message, a tool's result or a retry). A message sent
 * while the turn runs and not read yet changes nothing: the model goes on
 * with what it was doing (as lib/retry.ts reads it). */
export function turnState(view: SessionView | null | undefined, sending: boolean): TurnState {
  if (!view?.running_turn) return sending ? "sending" : null;
  if (view.pending_approvals.length) return "asking";
  const items: Item[] = view.items;
  // A turn's end before its next message (`turn_started` comes first) is a
  // wait too, not the last turn's answer.
  const last = items.findLast(
    (it) => !(it.type === "user" && it.queued && !it.queued.read) && ["assistant", "tool", "user", "callback", "retry", "turn_end"].includes(it.type),
  );
  if (last?.type === "assistant") return "answering";
  if (last?.type === "tool") return last.result ? "waiting" : "working";
  return "waiting";
}

/** Whole seconds since `since` (ms), never below 0. */
export const waited = (since: number, now: number) => Math.max(0, Math.floor((now - since) / 1000));

const WORDS = { sending: "conv.waiting", waiting: "conv.waiting", asking: "conv.asking", working: "conv.working", answering: "conv.answering" } as const;

export function TurnStatus(props: {
  state: TurnState;
  /** Changes whenever the wait starts again (a new event, a new send): the
   * count runs from when this tab saw that, on this tab's clock, so a
   * browser clock that is off cannot make it wrong. */
  wait: string;
  /** The turn (or the send) the screen reader was told about. */
  turn: string | null;
  name: string;
  mark?: ReactNode;
  /** Agents the last message called that have not had their turn yet (X.37). */
  left?: number;
  /** The live link to the server. Not "live": the line says so (the count
   * and the wait are not true then); it goes back when the link does. */
  link?: ConnState;
}) {
  const { state } = props;
  const [now, setNow] = useState(Date.now);
  const [since, setSince] = useState(() => ({ wait: props.wait, at: Date.now() }));
  if (since.wait !== props.wait) setSince({ wait: props.wait, at: Date.now() });
  const down = !!props.link && props.link !== "live";
  const counting = !down && (state === "waiting" || state === "sending");
  useEffect(() => {
    if (!counting) return;
    setNow(Date.now());
    const id = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(id);
  }, [counting, since.at]);
  // Said once per turn: the wait when it begins, "waiting for you" when an
  // approval comes; the steps between (a tool, the next wait) are not.
  const [said, setSaid] = useState<{ turn: string | null; words: string }>({ turn: null, words: "" });
  const tell = !state || !props.turn ? "" : state === "asking" ? "asking" : "waiting";
  const words = state ? t(WORDS[state], { name: props.name }) : "";
  const told = tell ? t(WORDS[tell as "asking" | "waiting"], { name: props.name }) : "";
  if (props.turn !== said.turn || (tell === "asking" && told !== said.words)) setSaid({ turn: props.turn, words: told });
  const s = waited(since.at, Math.max(now, since.at));
  return (
    <>
      {/* Always there, so a screen reader hears the wait when it begins (a
          live region inserted with its text is often missed). */}
      <span className="sr-only" role="status">
        {said.words}
      </span>
      {state && (
        <div
          className={`thinking turn-${down ? "down" : state}`}
          data-testid="turn-status"
          aria-hidden={down ? undefined : "true"}
          role={down ? "status" : undefined}
        >
          {props.mark}
          <span>{down ? t("conv.offline") : words}</span>
          {!down && !!props.left && <span className="waited">{t("conv.peersLeft", { n: String(props.left) })}</span>}
          {counting && s >= COUNT_FROM_S && <span className="waited">{t("conv.waited", { s: String(s) })}</span>}
        </div>
      )}
    </>
  );
}

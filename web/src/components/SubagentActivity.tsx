// What a sub-agent is doing, on its parent's `delegate` card: its latest
// tool call, how many it made, and its tokens once its turn ended. It
// follows the child's own session over the page's one socket, which checks
// access exactly as opening that chat does (the server's `may_see`), so the
// card shows nothing a person could not open themselves.
import { type RefObject, useContext, useEffect, useRef, useState, useSyncExternalStore } from "react";
import "../lib/i18nPages";
import { markedText } from "../lib/callText";
import { t } from "../lib/i18n";
import { LiveCtx } from "../lib/liveCtx";
import { shownName } from "../md/hidden";
import { costText, estimate, pricesSet } from "../lib/cost";
import type { Item, ModelInfo, SessionView } from "../types";
import { compactTokens } from "./ContextMeter";

type Tool = Extract<Item, { type: "tool" }>;

const asText = (v: unknown) => (typeof v === "string" ? v : "");

/** A call in a few words: its tool and what it works on. */
function callWords(it: Tool): string {
  const i = (it.input ?? {}) as Record<string, unknown>;
  const what =
    it.tool === "bash"
      ? asText(i.command)
      : it.tool === "grep" || it.tool === "glob"
        ? asText(i.pattern)
        : asText(i.path) || asText(i.query) || asText(i.url);
  // Marked as the card's own head marks it (hidden characters, runs of
  // blanks), on one line.
  const line = markedText(what.trim(), "cmd").replace(/\n/g, " ");
  const cut = line.length > 80 ? `${line.slice(0, 79)}…` : line;
  return cut ? `${shownName(it.tool)} ${cut}` : shownName(it.tool);
}

export interface Activity {
  tools: number;
  /** Input and output tokens, once a turn of it ended (null before). */
  tokens: number | null;
  /** Its latest call, in a few words. */
  last: string | null;
  running: boolean;
  waiting: boolean;
  /** Its cost estimate's words, when the server prices models (and given). */
  cost?: string | null;
}

/** Pure, for tests. */
export function activityOf(v: SessionView, models?: ModelInfo[]): Activity {
  let tools = 0;
  let last: Tool | null = null;
  for (const it of v.items) {
    if (it.type === "tool") {
      tools++;
      last = it;
    }
  }
  const tokens = v.usage.input_tokens + v.usage.output_tokens;
  return {
    tools,
    tokens: tokens > 0 ? tokens : null,
    last: last ? callWords(last) : null,
    running: v.running_turn !== null,
    waiting: v.pending_approvals.length > 0,
    cost: models && tokens > 0 && pricesSet(models) ? (costText(estimate(v.usage, models))?.text ?? null) : null,
  };
}

/** The words of the activity line. */
export function activityWords(a: Activity): string {
  const parts: string[] = [];
  if (a.tools > 0) parts.push(a.tools === 1 ? t("conv.childTools1") : t("conv.childTools", { n: String(a.tools) }));
  if (a.waiting) parts.push(t("conv.childWaiting"));
  else if (a.last) parts.push(t(a.running ? "conv.childNow" : "conv.childLast", { what: a.last }));
  if (a.tokens !== null) parts.push(t("conv.childTokens", { n: compactTokens(a.tokens) }));
  if (a.cost) parts.push(a.cost);
  return parts.length ? parts.join(" · ") : t("conv.childStarting");
}

/** Follow the child while it works; an ended one once the card is seen
 * (an old chat with many delegate cards does not replay every child's log
 * at once). */
export function useFollow(running: boolean): [RefObject<HTMLSpanElement | null>, boolean] {
  const ref = useRef<HTMLSpanElement>(null);
  const [seen, setSeen] = useState(false);
  useEffect(() => {
    if (running || seen) return;
    const el = ref.current;
    if (!el || typeof IntersectionObserver === "undefined") {
      setSeen(true);
      return;
    }
    const io = new IntersectionObserver((es) => {
      if (es.some((e) => e.isIntersecting)) setSeen(true);
    });
    io.observe(el);
    return () => io.disconnect();
  }, [running, seen]);
  return [ref, running || seen];
}

/** How long an ended child's replay must be still before its line is kept. */
const QUIET_MS = 1500;

/** A child whose turn is over, as far as its replayed log shows: its line
 * will not change again. */
function settled(v: SessionView): boolean {
  return v.running_turn === null && v.items.some((it) => it.type === "turn_end");
}

/** `running`: the parent's turn is live and still waits for this child.
 * An ended child is read once: its line is computed from the replay, kept,
 * and the subscription (and the folded log it holds) let go (review 98-1). */
export function SubagentActivity({
  sessionId,
  running,
  models,
}: {
  sessionId: string;
  running: boolean;
  models?: ModelInfo[];
}) {
  const live = useContext(LiveCtx);
  const [frozen, setFrozen] = useState<string | null>(null);
  const [ref, wanted] = useFollow(running);
  const follow = wanted && frozen === null;
  useEffect(() => {
    if (!live || !follow) return;
    live.watch(sessionId);
    return () => live.unwatch(sessionId);
  }, [live, follow, sessionId]);
  const version = () => live?.version ?? 0;
  useSyncExternalStore((l) => (live ? live.subscribe(l) : () => {}), version, version);
  const v = follow ? live?.view(sessionId) : undefined;
  // Nothing until its log arrives: an unknown or refused session shows no line.
  const words = v && v.head_seq > 0 ? activityWords(activityOf(v, models)) : null;
  const loaded = !running && !!v && v.head_seq > 0;
  const head = v?.head_seq ?? 0;
  useEffect(() => {
    if (!loaded || words === null) return;
    if (v && settled(v)) {
      setFrozen(words);
      return;
    }
    // A child whose log never shows its end (its turn was cut off): once
    // the replay has stopped moving, what it shows is all there is.
    const id = setTimeout(() => setFrozen(words), QUIET_MS);
    return () => clearTimeout(id);
  }, [loaded, words, head]);
  return (
    <span ref={ref} className="child-activity muted" aria-live="polite">
      {frozen ?? words}
    </span>
  );
}

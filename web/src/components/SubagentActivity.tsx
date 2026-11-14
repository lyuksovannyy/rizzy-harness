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
import type { Item, SessionView } from "../types";
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
}

/** Pure, for tests. */
export function activityOf(v: SessionView): Activity {
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
  };
}

/** The words of the activity line. */
export function activityWords(a: Activity): string {
  const parts: string[] = [];
  if (a.tools > 0) parts.push(a.tools === 1 ? t("conv.childTools1") : t("conv.childTools", { n: String(a.tools) }));
  if (a.waiting) parts.push(t("conv.childWaiting"));
  else if (a.last) parts.push(t(a.running ? "conv.childNow" : "conv.childLast", { what: a.last }));
  if (a.tokens !== null) parts.push(t("conv.childTokens", { n: compactTokens(a.tokens) }));
  return parts.length ? parts.join(" · ") : t("conv.childStarting");
}

/** Follow the child while it works; an ended one once the card is seen
 * (an old chat with many delegate cards does not replay every child's log
 * at once). */
function useFollow(running: boolean): [RefObject<HTMLSpanElement | null>, boolean] {
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

export function SubagentActivity({ sessionId, running }: { sessionId: string; running: boolean }) {
  const live = useContext(LiveCtx);
  const [ref, follow] = useFollow(running);
  useEffect(() => {
    if (!live || !follow) return;
    live.watch(sessionId);
    return () => live.unwatch(sessionId);
  }, [live, follow, sessionId]);
  const version = () => live?.version ?? 0;
  useSyncExternalStore((l) => (live ? live.subscribe(l) : () => {}), version, version);
  const v = follow ? live?.view(sessionId) : undefined;
  // Nothing until its log arrives: an unknown or refused session shows no line.
  const words = v && v.head_seq > 0 ? activityWords(activityOf(v)) : null;
  return (
    <span ref={ref} className="child-activity muted" aria-live="polite">
      {words}
    </span>
  );
}

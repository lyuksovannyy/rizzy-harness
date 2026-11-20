// A `delegate` card's two additions (X.41): the sub-agent's model next to
// its link, and its report open under the card as a message. The report is
// the quoted lines of the call's result (`harness_core::subagent::render`),
// unquoted here and shown by the same Markdown component as every model
// message: no HTML, no remote images, links show their host. The harness's
// own lines around it ("finished: done", the cut note) are never part of it.
import { useContext, useEffect, useState, useSyncExternalStore } from "react";
import { LiveCtx } from "../lib/liveCtx";
import { Markdown } from "./Markdown";
import { useFollow } from "./SubagentActivity";

const QUOTE = "| ";
const CUT_NOTE = "(The report was cut:";

/** The sub-agent's report from a `delegate` result: its quoted lines, not
 * the status line, a note or the unapplied-change text after them. A report
 * the harness cut in the middle has a `head` and a `tail` and, between them,
 * the harness's own note (`cut`), which is never part of either: only
 * `| `-quoted lines are the sub-agent's. */
export function childReport(output: string): { head: string; tail: string | null; cut: string | null } {
  const lines = output.split("\n").slice(2);
  const quoted = (l: string) => l.startsWith(QUOTE) || l === QUOTE.trimEnd();
  const run = (from: number): [string[], number] => {
    const got: string[] = [];
    let i = from;
    while (i < lines.length && quoted(lines[i]!)) got.push(lines[i++]!.slice(QUOTE.length));
    return [got, i];
  };
  // Leading blank lines go; the first line's own indentation stays (an
  // indented code block starts a report).
  const text = (ls: string[]) => ls.join("\n").replace(/^(?:[ \t]*\n)+/, "").trimEnd();
  const [head, at] = run(0);
  if (head.length > 0 && lines[at]?.startsWith(CUT_NOTE)) {
    const tail = text(run(at + 1)[0]);
    return { head: text(head), tail: tail || null, cut: lines[at]! };
  }
  const only = text(head);
  return { head: only === "(It wrote no report.)" ? "" : only, tail: null, cut: null };
}

/** How long a child's log may stay silent, once its call has a result,
 * before it is let go. While the call still runs the card keeps following
 * (a slow model's first reply can take minutes); a session the server says
 * it does not know (`Live.refused`) is let go at once. */
export const GIVE_UP_MS = 10_000;

/** The sub-agent's model, as its own chat's header names it: from the log
 * of that chat, read until the first event names it. `done`: the delegate
 * call has its result. */
export function ChildModel({ sessionId, running, done }: { sessionId: string; running: boolean; done: boolean }) {
  const live = useContext(LiveCtx);
  const [model, setModel] = useState<string | null>(null);
  const [ref, wanted] = useFollow(running);
  const [gaveUp, setGaveUp] = useState(false);
  const refused = !!live?.refused?.(sessionId);
  const follow = wanted && model === null && !gaveUp && !refused;
  useEffect(() => {
    if (!live || !follow) return;
    live.watch(sessionId);
    return () => live.unwatch(sessionId);
  }, [live, follow, sessionId]);
  // The timer runs only for a finished call whose child is still silent.
  useEffect(() => {
    if (!follow || !done) return;
    const id = setTimeout(() => setGaveUp(true), GIVE_UP_MS);
    return () => clearTimeout(id);
  }, [follow, done]);
  const version = () => live?.version ?? 0;
  useSyncExternalStore((l) => (live ? live.subscribe(l) : () => {}), version, version);
  const seen = follow ? live?.view(sessionId)?.meta?.model : undefined;
  useEffect(() => {
    if (seen) setModel(seen);
  }, [seen]);
  return (
    <span ref={ref} className="child-model mono muted" title="The model this sub-agent runs on">
      {model}
    </span>
  );
}

/** Long enough to cut: the card keeps the answer open but not a page of it. */
const LONG_CHARS = 900;
const LONG_LINES = 14;

/** The report as an open message, cut with "Show all" when long. What is cut
 * off cannot hold focus: a focus inside the clipped answer opens it. */
export function ChildAnswer({ output, agent }: { output: string; agent: string }) {
  const { head, tail, cut } = childReport(output);
  const [all, setAll] = useState(false);
  if (!head) return null;
  const whole = tail ? `${head}\n${tail}` : head;
  const long = whole.length > LONG_CHARS || whole.split("\n").length > LONG_LINES;
  const clipped = long && !all;
  return (
    <div className="child-answer">
      {/* Focus moving into what is cut off (a link reached by Tab) opens it. */}
      <div className={`child-answer-text${clipped ? " clipped" : ""}`} onFocus={() => setAll(true)}>
        <Markdown text={head} done from={agent} />
        {/* The harness's words, apart from both halves of the report. */}
        {cut && <p className="muted small child-cut">{cut}</p>}
        {tail && <Markdown text={tail} done from={agent} />}
      </div>
      {long && (
        <button type="button" className="link small" aria-expanded={all} onClick={() => setAll(!all)}>
          {all ? "Show less" : "Show all"}
        </button>
      )}
    </div>
  );
}

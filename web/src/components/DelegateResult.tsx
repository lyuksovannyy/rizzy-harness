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

/** The sub-agent's report from a `delegate` result: its quoted lines (a cut
 * report's head and tail, with the harness's note between them as `cut`),
 * not the status line, a note or the unapplied-change text after them. */
export function childReport(output: string): { text: string; cut: string | null } {
  const lines = output.split("\n").slice(2);
  const quoted = (l: string) => l.startsWith(QUOTE) || l === QUOTE.trimEnd();
  const run = (from: number): [string[], number] => {
    const got: string[] = [];
    let i = from;
    while (i < lines.length && quoted(lines[i]!)) got.push(lines[i++]!.slice(QUOTE.length));
    return [got, i];
  };
  const [head, at] = run(0);
  let cut: string | null = null;
  let tail: string[] = [];
  if (head.length > 0 && lines[at]?.startsWith(CUT_NOTE)) {
    cut = lines[at]!;
    tail = run(at + 1)[0];
  }
  const text = [...head, ...tail].join("\n").trim();
  return { text: text === "(It wrote no report.)" ? "" : text, cut };
}

/** The sub-agent's model, as its own chat's header names it: from the log
 * of that chat, read until the first event names it. */
export function ChildModel({ sessionId, running }: { sessionId: string; running: boolean }) {
  const live = useContext(LiveCtx);
  const [model, setModel] = useState<string | null>(null);
  const [ref, wanted] = useFollow(running);
  const follow = wanted && model === null;
  useEffect(() => {
    if (!live || !follow) return;
    live.watch(sessionId);
    return () => live.unwatch(sessionId);
  }, [live, follow, sessionId]);
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

/** The report as an open message, cut with "Show all" when long. */
export function ChildAnswer({ output, agent }: { output: string; agent: string }) {
  const { text, cut } = childReport(output);
  const [all, setAll] = useState(false);
  if (!text) return null;
  const long = text.length > LONG_CHARS || text.split("\n").length > LONG_LINES;
  return (
    <div className="child-answer">
      <div className={`child-answer-text${long && !all ? " clipped" : ""}`}>
        <Markdown text={text} done from={agent} />
      </div>
      {long && (
        <button type="button" className="link small" aria-expanded={all} onClick={() => setAll(!all)}>
          {all ? "Show less" : "Show all"}
        </button>
      )}
      {cut && <div className="muted small">{cut}</div>}
    </div>
  );
}

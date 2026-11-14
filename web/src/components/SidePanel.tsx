// Work running beside the conversation, on the right as Claude Desktop has
// it: ONE list with the turn's sub-agents on top, a separator, then the
// chat's background commands (docs/background-commands.md §12). A finished
// row stays for the latest turn so a person sees what ran; a background
// command from an earlier turn stays while it runs. Opens by itself when it
// first loads (the conversation loads this chunk on the first sub-agent or
// background command), can be closed, and shows a count when closed; on a
// narrow screen it is a drawer behind that button. Every row is a fold of
// this chat's own log (the server sent it only to its owner), and a child's
// activity comes through the same check as opening that chat.
import { PanelRightClose, PanelRightOpen } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { call } from "../api";
import { leftText, msLeft, reasonText } from "../lib/background";
import { showHidden, shownName } from "../md/hidden";
import type { SessionView } from "../types";
import { type AgentRow, type CommandRow, type RowState, sideItems } from "./sideItems";
import { SubagentActivity } from "./SubagentActivity";
import "./sidePanel.css";

/** A stop the node took but whose end has not come back may be asked again after this. */
const RETRY_AFTER_MS = 10000;

type Click = { state: "stopping"; at: number } | { state: "failed"; error: string };

const STATE_WORDS: Record<RowState, string> = {
  running: "running",
  waiting: "waiting for approval",
  done: "done",
  partial: "partly done",
  failed: "failed",
  stopped: "stopped",
};
/** The tool card's status colours (`.tool-status.s-*`). */
const STATE_CLASS: Record<RowState, string> = {
  running: "running",
  waiting: "waiting",
  done: "ok",
  partial: "waiting",
  failed: "failed",
  stopped: "denied",
};

function State({ state, words }: { state: RowState; words?: string }) {
  return <span className={`tool-status side-state s-${STATE_CLASS[state]}`}>{words ?? STATE_WORDS[state]}</span>;
}

function AgentItem({ row }: { row: AgentRow }) {
  return (
    <li className="side-row" data-kind="agent" data-call={row.call_id}>
      <div className="side-row-head">
        <span className="side-name">@{shownName(row.agent)}</span>
        <State state={row.state} />
      </div>
      {/* The model's text: shown as text, hidden characters marked. */}
      <div className="side-what ellipsis" title={showHidden(row.task)}>
        {showHidden(row.task)}
        {row.cut ? " …" : ""}
      </div>
      {row.child && (
        <div className="side-row-foot">
          <SubagentActivity sessionId={row.child} running={row.state === "running" || row.state === "waiting"} />
          <a className="link small" href={`#/s/${row.child}`}>
            Open its chat
          </a>
        </div>
      )}
    </li>
  );
}

export default function SidePanel(props: {
  sessionId: string;
  view: SessionView | null;
  /** The chat's node is known to be away (not merely not listed). */
  away: boolean;
  nodeLabel: string;
}) {
  const { sessionId, view, away } = props;
  const [open, setOpen] = useState(true);
  const [clicks, setClicks] = useState<Record<string, Click>>({});
  const [now, setNow] = useState(() => Date.now());
  const keep = useMemo(() => new Set(Object.keys(clicks)), [clicks]);
  const { agents, commands } = sideItems(view, keep);
  const runningCmds = commands.filter((c) => c.state === "running");
  const live = runningCmds.length > 0;
  // A coarse clock for "time left" while a command runs.
  useEffect(() => {
    if (!live) return;
    const id = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(id);
  }, [live]);
  // A command whose end arrived: what this tab clicked no longer matters.
  const endedKey = commands
    .filter((c) => c.ended && clicks[c.bg_id])
    .map((c) => c.bg_id)
    .join(",");
  useEffect(() => {
    if (!endedKey) return;
    setClicks((cs) => Object.fromEntries(Object.entries(cs).filter(([id]) => !endedKey.split(",").includes(id))));
  }, [endedKey]);

  const total = agents.length + commands.length;
  if (total === 0) return null;
  const active = agents.filter((a) => a.state === "running" || a.state === "waiting").length + runningCmds.length;
  const label = `Running beside the chat: ${active} active, ${total} in all`;

  if (!open)
    return (
      <button type="button" className="side-toggle" aria-label={`Open the side panel (${label})`} title={label} onClick={() => setOpen(true)}>
        <PanelRightOpen size={16} aria-hidden />
        <span className="side-count">{active > 0 ? active : total}</span>
      </button>
    );

  const set = (ids: string[], c: Click) => setClicks((cs) => ({ ...cs, ...Object.fromEntries(ids.map((id) => [id, c])) }));
  const stop = async (ids: string[], all: boolean) => {
    set(ids, { state: "stopping", at: Date.now() });
    try {
      await call("POST", all ? `/api/sessions/${sessionId}/background/stop` : `/api/sessions/${sessionId}/background/${ids[0]}/stop`);
    } catch (x) {
      set(ids, { state: "failed", error: x instanceof Error ? x.message : String(x) });
    }
  };
  const busy = (id: string) => {
    const c = clicks[id];
    return c?.state === "stopping" && now - c.at < RETRY_AFTER_MS;
  };
  const stoppable = runningCmds.filter((r) => !busy(r.bg_id));

  const command = (r: CommandRow) => {
    const c = clicks[r.bg_id];
    const stopping = c?.state === "stopping";
    const words = r.ended
      ? reasonText(r.ended.reason, r.ended.exit_code)
      : stopping
        ? "stopping…"
        : away
          ? "node away"
          : leftText(msLeft(r, now));
    return (
      <li key={r.bg_id} className="side-row" data-kind="command" data-bg={r.bg_id}>
        <div className="side-row-head">
          <span className="mono side-name">{showHidden(r.bg_id)}</span>
          <State state={r.state} />
        </div>
        {/* The model's text: shown as text, hidden characters marked. */}
        <code className="side-what ellipsis" title={showHidden(r.head)}>
          {showHidden(r.head)}
          {r.cut ? " …" : ""}
        </code>
        <div className="side-row-foot">
          <span className="muted small" role="status">
            {c?.state === "failed" ? <span className="error-text">{showHidden(c.error)}</span> : words}
          </span>
          {!r.ended && (
            <button
              type="button"
              className="danger small"
              aria-label={`Stop ${r.bg_id}`}
              disabled={away || busy(r.bg_id)}
              onClick={() => void stop([r.bg_id], false)}
            >
              {stopping && !busy(r.bg_id) ? "Stop again" : "Stop"}
            </button>
          )}
        </div>
      </li>
    );
  };

  return (
    <aside className="side-panel" aria-label="Running beside the chat">
      <div className="side-head">
        <span className="small">
          Running beside the chat <span className="muted">· {active} active</span>
        </span>
        <button type="button" className="icon small" aria-label="Close the side panel" title="Close" onClick={() => setOpen(false)}>
          <PanelRightClose size={16} aria-hidden />
        </button>
      </div>
      <ul className="side-rows">
        {agents.map((a) => (
          <AgentItem key={a.call_id} row={a} />
        ))}
        {agents.length > 0 && commands.length > 0 && <li role="separator" className="side-sep" />}
        {commands.length > 0 && (
          <li className="side-group small" data-kind="commands-head">
            <span>
              Background commands <span className="muted">· keep running when the turn stops</span>
            </span>
            {runningCmds.length >= 2 && (
              <button
                type="button"
                className="danger small"
                disabled={away || stoppable.length === 0}
                onClick={() => void stop(runningCmds.map((r) => r.bg_id), true)}
              >
                Stop all
              </button>
            )}
          </li>
        )}
        {commands.length > 0 && away && (
          <li className="side-away small" role="note">
            The node {showHidden(props.nodeLabel)} is away, so a stop cannot reach it. These end with the node or at their timeout.
          </li>
        )}
        {commands.map(command)}
      </ul>
    </aside>
  );
}

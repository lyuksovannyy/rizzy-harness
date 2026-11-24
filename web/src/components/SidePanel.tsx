// Work running beside the conversation, on the right as Claude Desktop has
// it: ONE list with the turn's sub-agents on top, a separator, then the
// chat's background commands (docs/background-commands.md §12). A finished
// row stays for the latest turn so a person sees what ran; a background
// command from an earlier turn stays while it runs. Opens by itself when it
// first loads (the conversation loads this chunk on the first sub-agent or
// background command), can be closed, and shows a count when closed; on a
// narrow screen it is a drawer behind that button. Every row is a fold of
// this chat's own log (the server sent it only to its owner), and a child's
// activity comes through the same check as opening that chat. Below them,
// the calls the person allowed for the rest of this chat (T51), each with
// Remove: they are this chat's live state, so they sit beside it rather
// than in Settings (Claude Desktop keeps "always allow" in its settings, far
// from the chat it applies to; ours is per chat, so it is listed with it).
import { PanelRightClose, PanelRightOpen } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { call } from "../api";
import { useMedia } from "../bg/hooks";
import { leftText, msLeft, reasonText } from "../lib/background";
import { showHidden, shownName } from "../md/hidden";
import type { ModelInfo, SessionView } from "../types";
import CommandOutput from "./CommandOutput";
import { type AgentRow, type CommandRow, type RowState, sideItems } from "./sideItems";
import { SubagentActivity } from "./SubagentActivity";
import "./sidePanel.css";

/** Every argument of a granted call, as the card shows them (review L3):
 * `key: value` lines, long text cut with its size, so two grants never look
 * the same. */
const GRANT_FIELD_MAX = 120;
function grantText(input: Record<string, unknown> | undefined): string {
  return Object.entries(input ?? {})
    .map(([k, v]) => {
      const t = typeof v === "string" ? v : JSON.stringify(v);
      const shown = t.length > GRANT_FIELD_MAX ? `${t.slice(0, GRANT_FIELD_MAX)}… (${t.length} characters)` : t;
      return `${k}: ${shown}`;
    })
    .join("\n");
}

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

/** A background sub-agent's Stop (X.24), as a command's: "stopping…" until
 * its end shows, the refusal on the row, and no button while the node is
 * away (a note instead). */
interface BgStop {
  away: boolean;
  click?: Click;
  busy: boolean;
  stop: (bg: string) => void;
}

function AgentItem({ row, models, bg }: { row: AgentRow; models?: ModelInfo[]; bg?: BgStop }) {
  return (
    <li className="side-row" data-kind="agent" data-call={row.call_id}>
      <div className="side-row-head">
        <span className="side-name">@{shownName(row.agent)}</span>
        {row.runs > 1 && <span className="small muted">{row.runs} runs</span>}
        <State state={row.state} />
      </div>
      {/* The model's text: shown as text, hidden characters marked. */}
      <div className="side-what ellipsis" title={showHidden(row.task)}>
        {showHidden(row.task)}
        {row.cut ? " …" : ""}
      </div>
      {row.child && (
        <div className="side-row-foot">
          <SubagentActivity
            sessionId={row.child}
            running={row.state === "running" || row.state === "waiting"}
            models={models}
          />
          <a className="link small" href={`#/s/${row.child}`}>
            Open its chat
          </a>
          {/* In the background (X.24): Stop ends it, as a command's Stop. */}
          {row.bg_id && bg && !bg.away && (
            <button type="button" className="danger small" aria-label={`Stop ${row.bg_id}`} disabled={bg.busy} onClick={() => bg.stop(row.bg_id!)}>
              {bg.click?.state === "stopping" && !bg.busy ? "Stop again" : bg.busy ? "stopping…" : "Stop"}
            </button>
          )}
        </div>
      )}
      {row.bg_id && bg && (bg.away || bg.click?.state === "failed") && (
        <div className="small muted" role="status">
          {bg.click?.state === "failed" ? (
            <span className="error-text">{showHidden(bg.click.error)}</span>
          ) : (
            "The node is away, so a stop cannot reach it: it ends with that node or at its time limit."
          )}
        </div>
      )}
    </li>
  );
}

/** Where the panel covers the chat (the shell's own narrow breakpoint). */
const NARROW = "(max-width: 820px)";

export default function SidePanel(props: {
  sessionId: string;
  view: SessionView | null;
  /** The chat's node is known to be away (not merely not listed). */
  away: boolean;
  nodeLabel: string;
  /** Another panel (a turn's changes) has the right side now: show only
   * the count button, whose click gives the side back (review D7). */
  yielded?: boolean;
  onReclaim?: () => void;
  /** The model list with its prices, for each sub-agent's cost. */
  models?: ModelInfo[];
}) {
  const { sessionId, view, away } = props;
  // Closed until asked for on a narrow screen, where it covers the chat: it
  // opens itself there only for work that is running. Wide: open, as before.
  const narrow = useMedia(NARROW);
  const [chosen, setOpen] = useState<boolean | null>(null);
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

  const grants = Object.entries(view?.chat_allows ?? {});
  const total = agents.length + commands.length + grants.length;
  if (total === 0) return null;
  const active = agents.filter((a) => a.state === "running" || a.state === "waiting").length + runningCmds.length;
  const open = chosen ?? (!narrow || active > 0);
  const label = `Running beside the chat: ${active} active, ${total} in all`;

  if (!open || props.yielded)
    return (
      <button
        type="button"
        className={props.yielded ? "side-toggle yielded" : "side-toggle"}
        aria-label={`Open the side panel (${label})`}
        title={label}
        onClick={() => {
          if (props.yielded) props.onReclaim?.();
          setOpen(true);
        }}
      >
        <PanelRightOpen size={16} aria-hidden />
        <span className="side-count">{active > 0 ? active : total}</span>
      </button>
    );

  const set = (ids: string[], c: Click) => setClicks((cs) => ({ ...cs, ...Object.fromEntries(ids.map((id) => [id, c])) }));
  const stop = async (ids: string[], all: boolean) => {
    set(ids, { state: "stopping", at: Date.now() });
    try {
      await call("POST", all ? `/api/sessions/${encodeURIComponent(sessionId)}/background/stop` : `/api/sessions/${encodeURIComponent(sessionId)}/background/${encodeURIComponent(ids[0] ?? "")}/stop`);
    } catch (x) {
      set(ids, { state: "failed", error: x instanceof Error ? x.message : String(x) });
    }
  };
  const removeGrant = async (id: string) => {
    set([`grant:${id}`], { state: "stopping", at: Date.now() });
    try {
      await call("DELETE", `/api/sessions/${encodeURIComponent(sessionId)}/allows/${encodeURIComponent(id)}`);
    } catch (x) {
      set([`grant:${id}`], { state: "failed", error: x instanceof Error ? x.message : String(x) });
    }
  };
  const busy = (id: string) => {
    const c = clicks[id];
    return c?.state === "stopping" && now - c.at < RETRY_AFTER_MS;
  };
  // Background sub-agents (X.24) are stopped with the commands.
  const runningKids = agents.filter((a) => a.bg_id);
  const allIds = [...runningCmds.map((r) => r.bg_id), ...runningKids.map((a) => a.bg_id!)];
  const stopAll = (
    <button type="button" className="danger small" disabled={away || allIds.every(busy)} onClick={() => void stop(allIds, true)}>
      Stop all
    </button>
  );

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
        <CommandOutput
          sessionId={sessionId}
          bg={r.bg_id}
          ended={!!r.ended}
          away={away}
          status={
            <span className="muted small" role="status">
              {c?.state === "failed" ? <span className="error-text">{showHidden(c.error)}</span> : words}
            </span>
          }
          stop={
            !r.ended && (
              <button
                type="button"
                className="danger small"
                aria-label={`Stop ${r.bg_id}`}
                disabled={away || busy(r.bg_id)}
                onClick={() => void stop([r.bg_id], false)}
              >
                {stopping && !busy(r.bg_id) ? "Stop again" : "Stop"}
              </button>
            )
          }
        />
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
        {runningKids.length > 0 && (
          <li className="side-group small" data-kind="bg-agents-head">
            <span>
              Sub-agents in the background <span className="muted">· keep running when the turn stops</span>
            </span>
            {commands.length === 0 && allIds.length >= 2 && stopAll}
          </li>
        )}
        {agents.map((a) => (
          <AgentItem
            key={a.call_id}
            row={a}
            models={props.models}
            bg={a.bg_id ? { away, click: clicks[a.bg_id], busy: busy(a.bg_id), stop: (id) => void stop([id], false) } : undefined}
          />
        ))}
        {agents.length > 0 && commands.length > 0 && <li role="separator" className="side-sep" />}
        {commands.length > 0 && (
          <li className="side-group small" data-kind="commands-head">
            <span>
              Background commands <span className="muted">· keep running when the turn stops</span>
            </span>
            {allIds.length >= 2 && stopAll}
          </li>
        )}
        {commands.length > 0 && away && (
          <li className="side-away small" role="note">
            The node {showHidden(props.nodeLabel)} is away, so a stop cannot reach it. These end with the node or at their timeout.
          </li>
        )}
        {commands.map(command)}
        {grants.length > 0 && (agents.length > 0 || commands.length > 0) && <li role="separator" className="side-sep" />}
        {grants.length > 0 && (
          <li className="side-group small" data-kind="grants-head">
            <span>
              Allowed in this chat <span className="muted">· run without asking (Ask and Accept edits)</span>
            </span>
          </li>
        )}
        {grants.map(([id, g]) => {
          const what = grantText(g.input);
          const c = clicks[`grant:${id}`];
          return (
            <li key={id} className="side-row" data-kind="grant" data-grant={id}>
              <div className="side-row-head">
                <span className="mono side-name">{showHidden(g.tool)}</span>
              </div>
              {/* The model's text: shown as text, hidden characters marked. */}
              <pre className="side-what side-grant">{showHidden(what)}</pre>
              <div className="side-row-foot">
                <span className="muted small" role="status">
                  {c?.state === "failed" ? <span className="error-text">{showHidden(c.error)}</span> : ""}
                </span>
                <button
                  type="button"
                  className="small"
                  aria-label={`Remove the allowance for ${g.tool}`}
                  disabled={c?.state === "stopping"}
                  onClick={() => void removeGrant(id)}
                >
                  Remove
                </button>
              </div>
            </li>
          );
        })}
      </ul>
    </aside>
  );
}

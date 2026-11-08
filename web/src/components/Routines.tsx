// Sidebar → Routines (docs/routines.md): saved instructions that start a chat
// on a schedule or when a webhook is called. The list, the editor
// (RoutineEditor) and each routine's runs (RoutineRuns) are one lazy chunk;
// the strings live here, not in the first-load catalog. Routines are private
// to their owner, so everything listed is the viewer's own.
import { ArrowLeft } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { api, HttpError } from "../api";
import { routineHash } from "../lib/routes";
import {
  browserZone,
  type Draft,
  isBad,
  type Routine,
  type RoutineList,
  routinesApi,
  ranNow,
  statusLabel,
  TEMPLATES,
  triggerSummary,
} from "../lib/routines";
import { when } from "../lib/when";
import type { Agent, NodeInfo } from "../types";
import { AgentMark } from "./AgentAvatar";
import { NodeBadge } from "./NodeBadge";
import type { RoutinesPageProps } from "./pages";
import { RoutineEditor } from "./RoutineEditor";
import { RoutineRuns } from "./RoutineRuns";

const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

/** Why a routine is under "Not running", or null when it runs. */
function notRunning(r: Routine, orphaned: boolean): string | null {
  if (r.blocked) return r.blocked;
  if (orphaned) return "You can no longer run routines here.";
  if (r.paused_reason === "failures") return "Paused after failing several runs in a row.";
  if (r.paused_reason === "cap") return "Paused: it reached a limit of this server.";
  if (r.paused_reason === "blocked") return `Paused: its last run was refused${r.last_run?.reason ? ` (${r.last_run.reason})` : ""}.`;
  return null;
}

/** Only deleting is left for a routine its owner may not run any more. */
const deleteOnly = (r: Routine, orphaned: boolean) => orphaned || !!r.blocked;

export default function Routines(props: RoutinesPageProps) {
  const { scope, live, navigate } = props;
  const [list, setList] = useState<RoutineList | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [zone, setZone] = useState(browserZone);
  // Bumped on every routines_changed, so an open Runs tab reloads too.
  const [version, setVersion] = useState(0);
  // A template's draft for the empty editor.
  const [seed, setSeed] = useState<Draft | null>(null);
  // Webhook secrets from the last save: shown once, never fetched again.
  const [secrets, setSecrets] = useState<Record<string, string>>({});
  // A routine the list does not hold (opened from another workspace, or just saved).
  const [extra, setExtra] = useState<Routine | null>(null);
  const [gone, setGone] = useState(false);

  const load = useCallback(() => {
    routinesApi
      .list(scope)
      .then((l) => {
        setList(l);
        setError(null);
      })
      .catch((e) => setError(message(e)));
    setVersion((v) => v + 1);
  }, [scope]);
  useEffect(() => {
    load();
    return live.onTopic("routines", load);
  }, [load, live]);
  // Times read in the user's own zone, which new schedules also start in.
  useEffect(() => {
    api
      .settings()
      .then((p) => p.timezone && setZone(p.timezone))
      .catch(() => {});
  }, []);

  const id = props.id;
  const listed = list && id && id !== "new" ? list.routines.find((r) => r.routine_id === id) : undefined;
  const routine = listed ?? (extra?.routine_id === id ? extra : null);
  useEffect(() => {
    setGone(false);
    if (!list || !id || id === "new" || listed || extra?.routine_id === id) return;
    routinesApi
      .get(id)
      .then(setExtra)
      .catch((e) => e instanceof HttpError && e.status === 404 && setGone(true));
  }, [id, list, listed, extra]);

  if (id !== null) {
    if (!list) return <Page>{error ? <div className="error-text">{error}</div> : <div className="muted">Loading…</div>}</Page>;
    if (gone) {
      return (
        <Page>
          <BackLink navigate={navigate} />
          <div className="muted">This routine no longer exists.</div>
        </Page>
      );
    }
    if (id !== "new" && !routine) return <Page><div className="muted">Loading…</div></Page>;
    if (props.tab === "runs" && routine) {
      return (
        <Page>
          <BackLink navigate={navigate} />
          <RoutineRuns key={routine.routine_id} routine={routine} zone={zone} navigate={navigate} version={version} />
        </Page>
      );
    }
    return (
      <Page>
        <BackLink navigate={navigate} />
        <RoutineEditor
          key={id}
          list={list}
          routine={routine}
          seed={id === "new" ? seed : null}
          agents={props.agents}
          nodes={props.nodes}
          scope={scope}
          zone={zone}
          webhooks={props.features.routines.webhooks}
          secrets={secrets}
          navigate={navigate}
          onSaved={(r, s) => {
            setExtra(r);
            setSecrets((old) => ({ ...old, ...s }));
            load();
            if (id === "new") navigate({ kind: "routines", id: r.routine_id });
          }}
          onDeleted={() => {
            load();
            navigate({ kind: "routines", id: null });
          }}
        />
      </Page>
    );
  }

  const create = (d: Draft | null) => {
    setSeed(d);
    navigate({ kind: "routines", id: "new" });
  };
  const running = list ? list.routines.filter((r) => !notRunning(r, list.orphaned)) : [];
  const stopped = list ? list.routines.filter((r) => notRunning(r, list.orphaned)) : [];
  const defaultAgent = props.agents[0]?.id ?? "general";
  return (
    <Page>
      <div className="agents-head">
        <h1>Routines</h1>
        {list?.can_create && (
          <button type="button" className="primary" onClick={() => create(null)}>
            New routine
          </button>
        )}
      </div>
      <p className="muted agents-intro">
        Instructions that start a chat on a schedule. Each run is a new chat in this workspace, started as you; only you see your
        routines.
      </p>
      {error && <div className="error-text">{error}</div>}
      {!list && !error && <div className="muted">Loading…</div>}
      {list && list.routines.length === 0 && (
        <div className="card stack routine-empty">
          {list.why ? (
            <p className="muted">{list.why}</p>
          ) : (
            <>
              <p>No routines yet.</p>
              {list.can_create && (
                <>
                  <p className="muted small">Start from an example, or make your own:</p>
                  <ul className="routine-templates">
                    {TEMPLATES.map((t) => (
                      <li key={t.name}>
                        <button type="button" onClick={() => create(t.draft(defaultAgent, zone, Date.now()))}>
                          <span className="routine-name">{t.name}</span>
                          <span className="muted small">{t.about}</span>
                        </button>
                      </li>
                    ))}
                  </ul>
                </>
              )}
            </>
          )}
        </div>
      )}
      {list && running.length > 0 && (
        <RoutineRows list={list} rows={running} agents={props.agents} nodes={props.nodes} zone={zone} navigate={navigate} onChanged={load} />
      )}
      {list && stopped.length > 0 && (
        <>
          <h2 className="routine-section">Not running</h2>
          <RoutineRows list={list} rows={stopped} agents={props.agents} nodes={props.nodes} zone={zone} navigate={navigate} onChanged={load} />
        </>
      )}
    </Page>
  );
}

function Page(props: { children: React.ReactNode }) {
  return (
    <section className="agents-page routines-page">
      <div className="column wide">{props.children}</div>
    </section>
  );
}

function BackLink({ navigate }: { navigate: RoutinesPageProps["navigate"] }) {
  return (
    <a
      className="agent-back link small row"
      href={routineHash(null)}
      onClick={(e) => {
        e.preventDefault();
        navigate({ kind: "routines", id: null });
      }}
    >
      <ArrowLeft size={14} aria-hidden="true" /> Routines
    </a>
  );
}

function RoutineRows(props: {
  list: RoutineList;
  rows: Routine[];
  agents: Agent[];
  nodes: NodeInfo[];
  zone: string;
  navigate: RoutinesPageProps["navigate"];
  onChanged: () => void;
}) {
  // A line under a row after Run now, a switch or a delete.
  const [notes, setNotes] = useState<Record<string, { text: string; bad: boolean }>>({});
  const note = (id: string, text: string, bad = false) => setNotes((n) => ({ ...n, [id]: { text, bad } }));
  const act = async (r: Routine, what: () => Promise<string | null>) => {
    try {
      const text = await what();
      if (text) note(r.routine_id, text);
      props.onChanged();
    } catch (e) {
      note(r.routine_id, message(e), true);
    }
  };
  const now = Date.now();
  return (
    <ul className="card agent-list agents-index routine-list">
      {props.rows.map((r) => {
        const agent = props.agents.find((a) => a.id === r.spec.agent_id);
        const stopped = notRunning(r, props.list.orphaned);
        const onlyDelete = deleteOnly(r, props.list.orphaned);
        const last = r.last_run;
        const on = r.enabled && !r.paused_reason;
        const face = (
          <>
            {agent ? <AgentMark agent={agent} size={32} /> : <span className="icon-slot" aria-hidden="true" />}
            <span className="agent-info">
              <span className="agent-name">
                {r.name}
                {r.unseen > 0 && (
                  <span className="dot unseen" role="img" aria-label={`${r.unseen} new ${r.unseen === 1 ? "result" : "results"}`} />
                )}
              </span>
              <span className="small muted routine-meta">
                <span>{triggerSummary(r.triggers, props.zone)}</span>
                <span className="tag">{r.spec.profile === "agentic" ? "Agentic" : "Chat"}</span>
                {r.spec.profile === "agentic" && r.spec.node_id && (
                  <NodeBadge nodeId={r.spec.node_id} node={props.nodes.find((n) => n.node_id === r.spec.node_id)} />
                )}
                {on && r.next_run_ms !== null && <span>Next {when(r.next_run_ms, now, props.zone)}</span>}
                {!r.enabled && !r.paused_reason && <span>Off</span>}
                {last && (
                  <span className={isBad(last.status) ? "routine-bad" : ""}>
                    Last: {statusLabel(last)} {when(last.started_ms, now, props.zone)}
                  </span>
                )}
              </span>
              {stopped && <span className="small routine-bad">{stopped}</span>}
              {notes[r.routine_id] && (
                <span className={`small ${notes[r.routine_id]!.bad ? "error-text" : "muted"}`} role="status">
                  {notes[r.routine_id]!.text}
                </span>
              )}
            </span>
          </>
        );
        return (
          <li key={r.routine_id} className="agent-item routine-item">
            {onlyDelete ? (
              <div className="agent-open">{face}</div>
            ) : (
              <button type="button" className="agent-open" onClick={() => props.navigate({ kind: "routines", id: r.routine_id })}>
                {face}
              </button>
            )}
            <span className="agent-actions routine-actions">
              {!onlyDelete && (
                <>
                  <input
                    type="checkbox"
                    className="switch"
                    aria-label={`Run ${r.name} on its schedule`}
                    title={on ? "On" : "Off"}
                    checked={on}
                    onChange={() => void act(r, async () => (await routinesApi.update(r.routine_id, { enabled: !on }), null))}
                  />
                  <button type="button" onClick={() => void act(r, async () => ranNow(await routinesApi.run(r.routine_id)))}>
                    Run now
                  </button>
                  <button
                    type="button"
                    disabled={!last}
                    onClick={() =>
                      last && props.navigate(last.session_id ? { kind: "session", id: last.session_id } : { kind: "routines", id: r.routine_id, tab: "runs" })
                    }
                  >
                    Open latest result
                  </button>
                </>
              )}
              {onlyDelete && (
                <button
                  type="button"
                  className="danger"
                  onClick={() => {
                    if (confirm(`Delete ${r.name}? The chats of its runs stay.`)) void act(r, async () => (await routinesApi.remove(r.routine_id), null));
                  }}
                >
                  Delete
                </button>
              )}
            </span>
          </li>
        );
      })}
    </ul>
  );
}

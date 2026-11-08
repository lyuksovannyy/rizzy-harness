// A routine's runs (docs/routines.md): when each started and what started
// it, how it ended, how long and how many tokens it took, the first sentence
// of its answer, and its chat. A row opens to show the prompt it was given
// and, for a webhook run, the event (untrusted data, shown as plain text).
// Unseen runs are bold; opening one's chat counts as seeing it.
import { Fragment, useEffect, useState } from "react";
import { markRunSeen } from "../lib/routineResults";
import {
  clock,
  duration,
  isBad,
  type Routine,
  routinesApi,
  type Run,
  type RunDetail,
  ranNow,
  statusLabel,
  tokens,
  TRIGGER_LABEL,
} from "../lib/routines";
import { when } from "../lib/when";
import type { RoutinesPageProps } from "./pages";
import { RoutineTabs } from "./RoutineEditor";

const message = (e: unknown) => (e instanceof Error ? e.message : String(e));
/** A run that started this much after its slot says when it was due. */
const LATE_MS = 60_000;

export function RoutineRuns(props: { routine: Routine; zone: string; navigate: RoutinesPageProps["navigate"]; version: number }) {
  const { routine, zone } = props;
  const [runs, setRuns] = useState<Run[] | null>(null);
  const [next, setNext] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const [open, setOpen] = useState<string | null>(null);
  const [details, setDetails] = useState<Record<string, RunDetail>>({});

  useEffect(() => {
    routinesApi
      .runs(routine.routine_id)
      .then((r) => {
        setRuns(r.runs);
        setNext(r.next);
      })
      .catch((e) => setError(message(e)));
  }, [routine.routine_id, props.version]);

  const more = async () => {
    try {
      const r = await routinesApi.runs(routine.routine_id, next);
      setRuns((old) => [...(old ?? []), ...r.runs]);
      setNext(r.next);
    } catch (e) {
      setError(message(e));
    }
  };
  const toggle = (run: Run) => {
    const id = open === run.run_id ? null : run.run_id;
    setOpen(id);
    if (id && !details[id]) {
      routinesApi
        .run_detail(routine.routine_id, id)
        .then((d) => setDetails((all) => ({ ...all, [id]: d })))
        .catch((e) => setError(message(e)));
    }
  };
  const openChat = (run: Run) => {
    if (!run.session_id) return;
    // A run still going is marked by its chat once it ends.
    if (!run.seen && run.ended_ms !== null) void markRunSeen(routine.routine_id, run.run_id).catch(() => {});
    props.navigate({ kind: "session", id: run.session_id });
  };

  const now = Date.now();
  return (
    <div className="routine-runs">
      <div className="agents-head">
        <h1>{routine.name}</h1>
        <button
          type="button"
          onClick={() =>
            void routinesApi
              .run(routine.routine_id)
              .then((res) => setNote(ranNow(res)))
              .catch((e) => setError(message(e)))
          }
        >
          Run now
        </button>
      </div>
      <RoutineTabs id={routine.routine_id} tab="runs" navigate={props.navigate} />
      {note && (
        <div className="muted small" role="status">
          {note}
        </div>
      )}
      {error && <div className="error-text">{error}</div>}
      {!runs && !error && <div className="muted">Loading…</div>}
      {runs && runs.length === 0 && <div className="muted">No runs yet. Run now starts one.</div>}
      {runs && runs.length > 0 && (
        <table className="users runs-table">
          <thead>
            <tr>
              <th>Started</th>
              <th>Trigger</th>
              <th>Status</th>
              <th>Duration</th>
              <th>Tokens</th>
              <th>Result</th>
              <th>Chat</th>
            </tr>
          </thead>
          <tbody>
            {runs.map((run) => {
              const late = run.scheduled_ms !== null && Math.abs(run.started_ms - run.scheduled_ms) >= LATE_MS;
              const d = details[run.run_id];
              return (
                <Fragment key={run.run_id}>
                  <tr className={run.seen ? "" : "unseen"}>
                    <td data-label="Started">
                      <button
                        type="button"
                        className="link"
                        title="Show what it was given"
                        aria-expanded={open === run.run_id}
                        onClick={() => toggle(run)}
                      >
                        {when(run.started_ms, now, zone)}
                      </button>
                      {late && <div className="muted small">scheduled {clock(run.scheduled_ms!, zone)}</div>}
                    </td>
                    <td data-label="Trigger">{TRIGGER_LABEL[run.trigger_kind] ?? run.trigger_kind}</td>
                    <td data-label="Status" className={isBad(run.status) ? "routine-bad" : ""} title={run.reason ?? undefined}>
                      {statusLabel(run)}
                      {run.reason && <div className="muted small">{run.reason}</div>}
                    </td>
                    <td data-label="Duration">{duration(run) ?? "—"}</td>
                    <td data-label="Tokens">{tokens(run.usage)}</td>
                    <td data-label="Result" className="runs-result">
                      {run.answer_preview ?? <span className="muted">—</span>}
                    </td>
                    <td data-label="Chat">
                      {run.chat_deleted ? (
                        <span className="muted small">chat deleted</span>
                      ) : run.session_id ? (
                        <button type="button" className="link" onClick={() => openChat(run)}>
                          Open chat
                        </button>
                      ) : (
                        <span className="muted">—</span>
                      )}
                    </td>
                  </tr>
                  {open === run.run_id && (
                    <tr className="run-detail">
                      <td colSpan={7}>
                        {!d ? (
                          <span className="muted small">Loading…</span>
                        ) : (
                          <div className="stack">
                            {d.warnings.length > 0 && <div className="small muted">{d.warnings.join(" · ")}</div>}
                            {d.prompt !== null ? (
                              <div className="field">
                                <span className="small muted">Prompt</span>
                                <pre className="routine-code">{d.prompt}</pre>
                              </div>
                            ) : (
                              <span className="muted small">
                                {d.chat_deleted ? "The prompt went with the deleted chat." : "No prompt was sent."}
                              </span>
                            )}
                            {d.event !== null && d.event !== undefined && (
                              <div className="field">
                                <span className="small muted">Event, from an external event</span>
                                <pre className="routine-code">{JSON.stringify(d.event, null, 2)}</pre>
                              </div>
                            )}
                          </div>
                        )}
                      </td>
                    </tr>
                  )}
                </Fragment>
              );
            })}
          </tbody>
        </table>
      )}
      {next && (
        <button type="button" className="routine-more" onClick={() => void more()}>
          Load more
        </button>
      )}
    </div>
  );
}

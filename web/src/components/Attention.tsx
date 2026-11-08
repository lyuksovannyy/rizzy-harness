// Chats waiting for the user (approvals, questions), then routine results
// not seen yet, shown under the bar until there are search results to show
// instead.
import { NODES_HASH } from "../lib/routes";
import { openResult, resultLabel } from "../lib/routineResults";
import { when } from "../lib/when";
import type { AttentionItem, NodeAttention, RoutineAttention } from "../types";

export function Attention(props: {
  items: AttentionItem[];
  nodes?: NodeAttention[];
  routines?: RoutineAttention[];
  onOpen: (id: string) => void;
  /** For the routine rows' times (prefs.timezone). */
  timeZone?: string | null;
}) {
  const nodes = props.nodes ?? [];
  const routines = props.routines ?? [];
  const waiting = props.items.length + nodes.length > 0;
  if (!waiting && routines.length === 0) return null;
  const now = Date.now();
  return (
    <>
      {waiting && (
        <div className="similar attention" aria-label="chats waiting for you">
          <div className="similar-label">Waiting for you</div>
          <ul>
            {nodes.map((n) => (
              <li key={`node-${n.setup_id}`}>
                <button type="button" onClick={() => (location.hash = NODES_HASH)}>
                  <span className="similar-title ellipsis">Server {n.host}</span>
                  <span className="similar-snippet ellipsis">{n.problems.map((p) => p.detail).join("; ")}</span>
                </button>
              </li>
            ))}
            {props.items.map((a) => (
              <li key={a.session_id}>
                <button type="button" onClick={() => props.onOpen(a.session_id)}>
                  <span className="similar-title ellipsis">{a.title}</span>
                  <span className="similar-snippet ellipsis">
                    {a.reason === "approval" ? "Needs your approval" : "Asked you a question"}
                    {a.parent_id ? " (sub-task)" : ""}
                  </span>
                </button>
              </li>
            ))}
          </ul>
        </div>
      )}
      {routines.length > 0 && (
        <div className="similar routine-results" aria-label="routine results">
          <div className="similar-label">Routine results</div>
          <ul>
            {routines.map((r) => (
              <li key={`${r.kind}-${r.routine_id}`}>
                <button type="button" className={r.kind === "result" ? "" : "bad"} onClick={() => openResult(r)}>
                  <span className="similar-title ellipsis">
                    {r.name} · {resultLabel(r)}
                    {r.unseen > 1 && <span className="muted"> · {r.unseen} new</span>}
                  </span>
                  <span className="similar-age muted">{when(r.ts_ms, now, props.timeZone)}</span>
                  {r.reason && r.kind !== "result" && <span className="similar-snippet ellipsis muted">{r.reason}</span>}
                </button>
              </li>
            ))}
          </ul>
        </div>
      )}
    </>
  );
}

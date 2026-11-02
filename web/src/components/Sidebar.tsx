import type { ConnState } from "../live";
import type { NodeInfo, SessionRow } from "../types";
import { NodeBadge } from "./NodeBadge";

function ago(ms: number): string {
  const s = Math.max(0, Math.round((Date.now() - ms) / 1000));
  if (s < 60) return `${s}s`;
  if (s < 3600) return `${Math.round(s / 60)}m`;
  if (s < 86400) return `${Math.round(s / 3600)}h`;
  return `${Math.round(s / 86400)}d`;
}

export function Sidebar(props: {
  sessions: SessionRow[];
  nodes: NodeInfo[];
  selected: string | null;
  conn: ConnState;
  onSelect: (id: string) => void;
  onNew: () => void;
  onLogout: () => void;
}) {
  const nodeById = new Map(props.nodes.map((n) => [n.node_id, n]));
  return (
    <aside className="sidebar">
      <div className="sidebar-head">
        <span className="brand">harness</span>
        <span className={`conn conn-${props.conn}`} title={`connection: ${props.conn}`} />
      </div>
      <button className="primary wide" onClick={props.onNew}>
        + New session
      </button>
      <nav className="session-list">
        {props.sessions.length === 0 && <div className="muted pad">No sessions yet.</div>}
        {props.sessions.map((s) => (
          <button
            key={s.session_id}
            className={`session-item ${s.session_id === props.selected ? "active" : ""}`}
            onClick={() => props.onSelect(s.session_id)}
          >
            <div className="session-title">
              {s.running && <span className="dot running" title="running" />}
              <span className="ellipsis">{s.title || "Untitled"}</span>
            </div>
            <div className="session-meta">
              <NodeBadge node={nodeById.get(s.node_id)} nodeId={s.node_id} />
              <span className="ellipsis mono">{s.cwd.split("/").slice(-2).join("/")}</span>
              <span className="muted">{ago(s.updated_ms)}</span>
            </div>
          </button>
        ))}
      </nav>
      <div className="sidebar-foot">
        <span className="muted">{props.nodes.filter((n) => n.connected).length} node(s) online</span>
        <button className="link" onClick={props.onLogout}>
          Log out
        </button>
      </div>
    </aside>
  );
}

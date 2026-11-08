import { Bot, Loader2, PanelLeftClose, Search, Settings, SquarePen, WifiOff } from "lucide-react";
import { APP_TITLE } from "../lib/brand";
import { useEffect, useState } from "react";
import { api } from "../api";
import type { ConnState } from "../live";
import { t } from "../lib/i18n";
import type { Me, NodeInfo, SearchHit, SessionRow, WorkspaceList } from "../types";
import { Highlight } from "./Highlight";
import { NodeBadge } from "./NodeBadge";
import { WorkspaceSwitcher } from "./WorkspaceSwitcher";

export function Sidebar(props: {
  me: Me;
  sessions: SessionRow[];
  nodes: NodeInfo[];
  selected: string | null;
  conn: ConnState;
  /** For the switcher; null until loaded. */
  workspaces: WorkspaceList | null;
  onSwitch: (id: string) => Promise<void>;
  onWorkspaceSettings: () => void;
  onSelect: (id: string) => void;
  onNew: () => void;
  /** The Agents page; `agentsOpen` while it shows. */
  onAgents: () => void;
  agentsOpen: boolean;
  onSettings: () => void;
  onLogout: () => void;
  onCollapse: () => void;
}) {
  const [query, setQuery] = useState("");
  const [searching, setSearching] = useState(false);
  const [hits, setHits] = useState<SearchHit[] | null>(null);
  const nodeById = new Map(props.nodes.map((n) => [n.node_id, n]));
  const q = query.trim();
  const workspaceId = props.me.workspace?.workspace_id ?? null;
  useEffect(() => {
    if (!q) {
      setHits(null);
      return;
    }
    const ctl = new AbortController();
    const timer = setTimeout(() => {
      api
        .search(q, false, ctl.signal, workspaceId)
        .then((r) => setHits(r.results))
        .catch(() => {});
    }, 150);
    return () => {
      clearTimeout(timer);
      ctl.abort();
    };
  }, [q, workspaceId]);
  const shown = hits === null ? props.sessions : [];

  return (
    <aside className="sidebar">
      <div className="sidebar-head">
        <span className="brand">{APP_TITLE}</span>
        {/* Nothing while connected; an icon only when something is wrong. */}
        {props.conn !== "live" && (
          <span className={`conn-icon conn-${props.conn}`} title={`connection: ${props.conn}`} role="status">
            {props.conn === "offline" ? <WifiOff size={14} /> : <Loader2 size={14} className="spin" />}
          </span>
        )}
        <span className="spacer" />
        <button
          className="icon"
          aria-label={t("nav.search")}
          title={t("nav.search")}
          onClick={() => setSearching((v) => !v)}
        >
          <Search size={18} />
        </button>
        <button className="icon" aria-label={t("nav.collapse")} title={t("nav.collapse")} onClick={props.onCollapse}>
          <PanelLeftClose size={18} />
        </button>
      </div>
      {props.me.workspace && (
        <WorkspaceSwitcher
          active={props.me.workspace}
          list={props.workspaces}
          onSwitch={props.onSwitch}
          onSettings={props.onWorkspaceSettings}
        />
      )}
      <button className="nav-item new-chat" onClick={props.onNew}>
        <SquarePen size={16} /> {t("nav.newChat")}
      </button>
      <button className={`nav-item ${props.agentsOpen ? "active" : ""}`} aria-current={props.agentsOpen ? "page" : undefined} onClick={props.onAgents}>
        <Bot size={16} /> {t("nav.agents")}
      </button>
      {searching && (
        <input
          className="sidebar-search"
          autoFocus
          placeholder={t("nav.search")}
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={(e) => e.key === "Escape" && (setQuery(""), setSearching(false))}
        />
      )}
      <div className="section-label">{hits === null ? t("nav.recents") : t("nav.search")}</div>
      <nav className="session-list">
        {hits !== null && hits.length === 0 && <div className="muted pad">{t("nav.noMatches")}</div>}
        {hits?.map((h) => (
          <button
            key={h.session_id}
            className={`session-item hit ${h.session_id === props.selected ? "active" : ""}`}
            onClick={() => props.onSelect(h.session_id)}
            title={h.title}
          >
            <span className="ellipsis">{h.title}</span>
            <span className="ellipsis muted small">
              <Highlight text={h.snippet} />
            </span>
          </button>
        ))}
        {hits === null && shown.length === 0 && <div className="muted pad">{t("nav.noChats")}</div>}
        {shown.map((s) => (
          <button
            key={s.session_id}
            className={`session-item ${s.session_id === props.selected ? "active" : ""}`}
            onClick={() => props.onSelect(s.session_id)}
            title={s.title}
          >
            {s.running && <span className="dot running" title="running" />}
            <span className="ellipsis grow">{s.title || t("session.untitled")}</span>
            {s.profile === "agentic" && <NodeBadge node={nodeById.get(s.node_id)} nodeId={s.node_id} />}
          </button>
        ))}
      </nav>
      <div className="sidebar-foot">
        <button className="nav-item user" onClick={props.onSettings} title={t("nav.settings")}>
          <span className="avatar">{props.me.username.slice(0, 1).toUpperCase()}</span>
          <span className="ellipsis grow">
            {props.me.username}
            {props.me.role === "admin" && <span className="muted"> · {t("nav.serverAdmin")}</span>}
          </span>
          <Settings size={16} className="muted" />
        </button>
        <button className="link" onClick={props.onLogout}>
          {t("nav.logout")}
        </button>
      </div>
    </aside>
  );
}

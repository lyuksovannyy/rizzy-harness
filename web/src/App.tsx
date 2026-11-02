import { useCallback, useEffect, useState, useSyncExternalStore } from "react";
import { api, HttpError } from "./api";
import { Live } from "./live";
import type { NodeInfo, SessionRow } from "./types";
import { Login } from "./components/Login";
import { Sidebar } from "./components/Sidebar";
import { SessionPane } from "./components/SessionPane";
import { NewSession } from "./components/NewSession";

function useHashRoute(): [string | null, (id: string | null) => void] {
  const read = () => {
    const m = location.hash.match(/^#\/s\/([0-9a-f-]{36})$/);
    return m ? m[1]! : null;
  };
  const [id, setId] = useState<string | null>(read);
  useEffect(() => {
    const on = () => setId(read());
    window.addEventListener("hashchange", on);
    return () => window.removeEventListener("hashchange", on);
  }, []);
  const go = (next: string | null) => {
    location.hash = next ? `#/s/${next}` : "#/";
  };
  return [id, go];
}

export function App() {
  const [authed, setAuthed] = useState<boolean | null>(null);
  useEffect(() => {
    api
      .me()
      .then(() => setAuthed(true))
      .catch((e) => setAuthed(e instanceof HttpError && e.status === 401 ? false : false));
  }, []);
  if (authed === null) return <div className="splash">Loading…</div>;
  if (!authed) return <Login onDone={() => setAuthed(true)} />;
  return <Shell onLogout={() => setAuthed(false)} />;
}

function Shell({ onLogout }: { onLogout: () => void }) {
  const [live] = useState(() => new Live());
  const [sessions, setSessions] = useState<SessionRow[]>([]);
  const [nodes, setNodes] = useState<NodeInfo[]>([]);
  const [selected, select] = useHashRoute();
  const [creating, setCreating] = useState(false);

  const refreshSessions = useCallback(() => {
    api
      .sessions()
      .then((r) => setSessions(r.sessions))
      .catch((e) => e instanceof HttpError && e.status === 401 && onLogout());
  }, [onLogout]);
  const refreshNodes = useCallback(() => {
    api
      .nodes()
      .then((r) => setNodes(r.nodes))
      .catch(() => {});
  }, []);

  useEffect(() => {
    live.onSessionsChanged = refreshSessions;
    live.onNodesChanged = refreshNodes;
    refreshSessions();
    refreshNodes();
    return () => live.close();
  }, [live, refreshSessions, refreshNodes]);

  useSyncExternalStore(
    (l) => live.subscribe(l),
    () => live.version,
  );

  const logout = async () => {
    await api.logout().catch(() => {});
    onLogout();
  };

  const current = sessions.find((s) => s.session_id === selected) ?? null;

  return (
    <div className="shell">
      <Sidebar
        sessions={sessions}
        nodes={nodes}
        selected={selected}
        conn={live.state}
        onSelect={(id) => {
          setCreating(false);
          select(id);
        }}
        onNew={() => setCreating(true)}
        onLogout={logout}
      />
      <main className="main">
        {creating ? (
          <NewSession
            nodes={nodes}
            onCancel={() => setCreating(false)}
            onCreated={(id) => {
              setCreating(false);
              refreshSessions();
              select(id);
            }}
          />
        ) : selected ? (
          <SessionPane key={selected} live={live} sessionId={selected} row={current} nodes={nodes} />
        ) : (
          <div className="empty">
            <h2>No session selected</h2>
            <p>Pick a session on the left, or start a new one.</p>
            <button className="primary" onClick={() => setCreating(true)}>
              New session
            </button>
          </div>
        )}
      </main>
    </div>
  );
}

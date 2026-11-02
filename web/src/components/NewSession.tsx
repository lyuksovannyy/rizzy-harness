import { useState } from "react";
import { api } from "../api";
import type { NodeInfo } from "../types";

const LAST_CWD = "harness.lastCwd";

export function NewSession(props: {
  nodes: NodeInfo[];
  onCancel: () => void;
  onCreated: (id: string) => void;
}) {
  const online = props.nodes.filter((n) => n.connected);
  const [cwd, setCwd] = useState(() => localStorage.getItem(LAST_CWD) ?? "");
  const [title, setTitle] = useState("");
  const [node, setNode] = useState(online[0]?.node_id ?? "");
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    setBusy(true);
    setErr(null);
    try {
      const r = await api.createSession({
        cwd: cwd.trim(),
        title: title.trim() || undefined,
        node_id: node || undefined,
      });
      localStorage.setItem(LAST_CWD, cwd.trim());
      props.onCreated(r.session_id);
    } catch (x) {
      setErr(x instanceof Error ? x.message : String(x));
    } finally {
      setBusy(false);
    }
  };

  return (
    <form className="new-session card" onSubmit={submit}>
      <h2>New session</h2>
      <label>
        Node
        <select value={node} onChange={(e) => setNode(e.target.value)}>
          {online.map((n) => (
            <option key={n.node_id} value={n.node_id}>
              {n.embedded ? `local (${n.hostname})` : n.hostname}
            </option>
          ))}
        </select>
      </label>
      <label>
        Working directory on that node
        <input
          className="mono"
          autoFocus
          placeholder="/home/me/project"
          value={cwd}
          onChange={(e) => setCwd(e.target.value)}
        />
      </label>
      <label>
        Title <span className="muted">(optional)</span>
        <input value={title} onChange={(e) => setTitle(e.target.value)} />
      </label>
      {err && <div className="error-text">{err}</div>}
      <div className="row">
        <button type="button" onClick={props.onCancel}>
          Cancel
        </button>
        <button className="primary" disabled={busy || !cwd.trim() || !node}>
          Create
        </button>
      </div>
    </form>
  );
}

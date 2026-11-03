// Settings → Nodes (admin): worker machines, adding one, revoking one.
import { useCallback, useEffect, useState } from "react";
import { api } from "../api";
import type { NodeInfo, RemoteNode } from "../types";
import { ServerSetup } from "./ServerSetup";

export function Nodes(props: { nodes: NodeInfo[] }) {
  const [remote, setRemote] = useState<RemoteNode[]>([]);
  const [listener, setListener] = useState<{ port: number; fingerprint: string } | null>(null);
  const [join, setJoin] = useState<{ token: string; port: number; fingerprint: string; expires_in_s: number } | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const load = useCallback(() => {
    api
      .remoteNodes()
      .then((r) => {
        setRemote(r.nodes);
        setListener(r.listener);
      })
      .catch((x) => setErr(String(x)));
  }, []);
  useEffect(load, [load, props.nodes]);
  const host = location.hostname || "this-host";
  const embedded = props.nodes.filter((n) => n.embedded);

  return (
    <div className="stack">
      <div className="card stack">
        <h3>Worker nodes</h3>
        <p className="muted">
          Agentic sessions run on a node: this machine, or another one that joined. Each node's own node.toml decides
          what it allows; the master can only ask for less.
        </p>
        <table className="users">
          <thead>
            <tr>
              <th>Node</th>
              <th>Host</th>
              <th>Status</th>
              <th />
            </tr>
          </thead>
          <tbody>
            {embedded.map((n) => (
              <tr key={n.node_id}>
                <td className="mono">{n.node_id}</td>
                <td>{n.hostname}</td>
                <td>{n.connected ? "online" : "offline"} · built in ({n.profile})</td>
                <td />
              </tr>
            ))}
            {remote.map((n) => (
              <tr key={n.node_id}>
                <td className="mono">{n.node_id}</td>
                <td>
                  {n.hostname} <span className="muted small mono">{n.key_fingerprint}</span>
                </td>
                <td>{n.revoked ? "revoked" : n.connected ? "online" : "offline"}</td>
                <td>
                  {!n.revoked && (
                    <button
                      type="button"
                      className="danger"
                      onClick={async () => {
                        if (!confirm(`Revoke ${n.node_id}? It is disconnected and its key stops working.`)) return;
                        await api.revokeNode(n.node_id).catch((x) => setErr(String(x)));
                        load();
                      }}
                    >
                      Revoke
                    </button>
                  )}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      <div className="card stack">
        <h3>Set up a server over SSH</h3>
        <ServerSetup onDone={load} />
      </div>
      <div className="card stack">
        <h3>Add a node by hand</h3>
        {!listener ? (
          <p className="muted">
            Remote nodes are off. Restart the server with <code>--node-listen 0.0.0.0:7443</code> (TLS with a pinned
            certificate; only joined nodes can connect).
          </p>
        ) : (
          <>
            <p className="muted">
              On the other machine, install the same binary, then run these two commands. The code works once and
              expires in 15 minutes.
            </p>
            {join ? (
              <pre className="mono join-cmd">
                {`harness node join wss://${host}:${join.port} \\\n  --token ${join.token} \\\n  --fingerprint ${join.fingerprint}\nharness node run`}
              </pre>
            ) : (
              <div className="row">
                <button
                  type="button"
                  className="primary"
                  onClick={() =>
                    api
                      .joinToken()
                      .then(setJoin)
                      .catch((x) => setErr(String(x)))
                  }
                >
                  Create join command
                </button>
              </div>
            )}
            <div className="muted small">
              If this machine is reached by another name, replace {host} in the URL. The fingerprint stays the same.
            </div>
          </>
        )}
        {err && <div className="error-text">{err}</div>}
      </div>
    </div>
  );
}

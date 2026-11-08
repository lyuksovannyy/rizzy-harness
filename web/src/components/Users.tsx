import { useCallback, useEffect, useState } from "react";
import { api } from "../api";
import type { Me, Role, User } from "../types";

/** Admin-only account management. Admins never see other users' chats. */
export function Users({ me }: { me: Me }) {
  const [users, setUsers] = useState<User[]>([]);
  const [err, setErr] = useState<string | null>(null);
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [role, setRole] = useState<Role>("member");
  const [agentic, setAgentic] = useState(false);

  const load = useCallback(() => {
    api
      .users()
      .then((r) => setUsers(r.users))
      .catch((e) => setErr(e instanceof Error ? e.message : String(e)));
  }, []);
  useEffect(load, [load]);

  const run = async (f: () => Promise<unknown>) => {
    setErr(null);
    try {
      await f();
      load();
    } catch (x) {
      setErr(x instanceof Error ? x.message : String(x));
    }
  };

  const create = (e: React.FormEvent) => {
    e.preventDefault();
    void run(async () => {
      await api.createUser({ username: username.trim(), password, role, can_agentic: agentic });
      setUsername("");
      setPassword("");
      setAgentic(false);
      setRole("member");
    });
  };

  return (
    <section className="stack">
      <p className="muted">
        Only admins create accounts. New users get a temporary password and must change it at first
        login. Agentic access runs shell commands on worker nodes: grant it only to people you would
        give a shell.
      </p>
      {err && <div className="error-text">{err}</div>}
      <table className="users">
        <thead>
          <tr>
            <th>User</th>
            <th>Role</th>
            <th>Agentic</th>
            <th>Status</th>
            <th />
          </tr>
        </thead>
        <tbody>
          {users.map((u) => {
            const self = u.user_id === me.user_id;
            return (
              <tr key={u.user_id} className={u.disabled ? "muted" : ""}>
                <td>
                  {u.username}
                  {self && <span className="muted"> (you)</span>}
                </td>
                <td>
                  <select
                    aria-label={`Server role of ${u.username}`}
                    value={u.role}
                    disabled={self}
                    onChange={(e) => run(() => api.updateUser(u.user_id, { role: e.target.value as Role }))}
                  >
                    <option value="member">User</option>
                    <option value="admin">Server admin</option>
                  </select>
                </td>
                <td>
                  <input
                    type="checkbox"
                    aria-label={`agentic access for ${u.username}`}
                    checked={u.can_agentic}
                    disabled={self || u.role === "admin"}
                    onChange={(e) => run(() => api.updateUser(u.user_id, { can_agentic: e.target.checked }))}
                  />
                </td>
                <td>
                  {u.disabled ? "disabled" : u.must_change_password ? "temporary password" : "active"}
                </td>
                <td>
                  {!self && (
                    <div className="row end">
                      <button
                        onClick={() => {
                          const pw = prompt(`New temporary password for ${u.username} (10+ characters):`);
                          if (pw) void run(() => api.updateUser(u.user_id, { password: pw }));
                        }}
                      >
                        Reset password
                      </button>
                      <button onClick={() => run(() => api.updateUser(u.user_id, { disabled: !u.disabled }))}>
                        {u.disabled ? "Enable" : "Disable"}
                      </button>
                      <button
                        className="danger"
                        onClick={() => {
                          if (confirm(`Delete ${u.username}? Their chats stay on disk but become inaccessible.`))
                            void run(() => api.deleteUser(u.user_id));
                        }}
                      >
                        Delete
                      </button>
                    </div>
                  )}
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>

      <form className="card" onSubmit={create}>
        <h3>Add user</h3>
        <div className="row">
          <input placeholder="username" value={username} onChange={(e) => setUsername(e.target.value)} />
          <input
            type="text"
            placeholder="temporary password (10+)"
            autoComplete="off"
            value={password}
            onChange={(e) => setPassword(e.target.value)}
          />
          <select aria-label="Server role" value={role} onChange={(e) => setRole(e.target.value as Role)}>
            <option value="member">User</option>
            <option value="admin">Server admin</option>
          </select>
          <label className="check">
            <input
              type="checkbox"
              checked={agentic || role === "admin"}
              disabled={role === "admin"}
              onChange={(e) => setAgentic(e.target.checked)}
            />
            agentic access
          </label>
          <button className="primary" disabled={!username.trim() || password.length < 10}>
            Create
          </button>
        </div>
      </form>
    </section>
  );
}

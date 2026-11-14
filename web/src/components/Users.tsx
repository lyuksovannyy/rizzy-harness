import { useCallback, useEffect, useRef, useState } from "react";
import { pageApi } from "../apiPages";
import type { Me, Role, User } from "../types";
import { LEVEL_LABEL, LevelSelect } from "./LevelSelect";

/** A temporary password the server just made (AUDIT L14). It lives only in
 * this component's state, which closing the dialog clears: never in a
 * store, a toast, the log or a URL. */
interface Shown {
  username: string;
  password: string;
}

function TempPassword({ shown, onClose }: { shown: Shown; onClose: () => void }) {
  const dialog = useRef<HTMLDialogElement>(null);
  const [copied, setCopied] = useState(false);
  useEffect(() => {
    const d = dialog.current;
    if (d && typeof d.showModal === "function" && !d.open) d.showModal();
  }, []);
  const copy = () =>
    navigator.clipboard
      ?.writeText(shown.password)
      .then(() => setCopied(true))
      .catch(() => {});
  return (
    <dialog ref={dialog} className="workspace-dialog stack" aria-label="Temporary password" onClose={onClose}>
      <p>
        Temporary password for <b>{shown.username}</b>. They must change it when they first sign in.
      </p>
      <code className="temp-password">{shown.password}</code>
      <p className="muted">It will not be shown again: copy it now and give it to them privately.</p>
      <div className="row end">
        <button type="button" onClick={() => void copy()}>
          {copied ? "Copied" : "Copy"}
        </button>
        <button type="button" className="primary" autoFocus onClick={onClose}>
          Done
        </button>
      </div>
    </dialog>
  );
}

/** Admin-only account management. Admins never see other users' chats. */
export function Users({ me }: { me: Me }) {
  const [users, setUsers] = useState<User[]>([]);
  const [err, setErr] = useState<string | null>(null);
  const [username, setUsername] = useState("");
  const [role, setRole] = useState<Role>("member");
  const [shown, setShown] = useState<Shown | null>(null);

  const load = useCallback(() => {
    pageApi
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
      const u = await pageApi.createUser({ username: username.trim(), role });
      setShown({ username: u.username, password: u.password });
      setUsername("");
      setRole("member");
    });
  };

  return (
    <section className="stack">
      <p className="muted">
        Only admins create accounts. The server makes each new user a temporary password, shown to you
        once, which they must change at first login. An account's level is the most it may run in any
        workspace (and what it runs in its Personal one): none, container, sandbox, or host, a shell on
        a node with no sandbox. Every new account starts at none; a server admin's is always host.
      </p>
      {err && <div className="error-text">{err}</div>}
      {shown && <TempPassword shown={shown} onClose={() => setShown(null)} />}
      <table className="users">
        <thead>
          <tr>
            <th>User</th>
            <th>Role</th>
            <th>Level</th>
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
                    onChange={(e) => {
                      const role = e.target.value as Role;
                      // A server admin's level is host only while they are one.
                      if (
                        u.role === "admin" &&
                        role === "member" &&
                        !confirm(
                          `Make ${u.username} a user? Their level will then be ${LEVEL_LABEL[u.exec_ceiling_stored]}; set it first if that is not what they should have.`,
                        )
                      )
                        return;
                      void run(() => pageApi.updateUser(u.user_id, { role }));
                    }}
                  >
                    <option value="member">User</option>
                    <option value="admin">Server admin</option>
                  </select>
                </td>
                <td>
                  <LevelSelect
                    label={u.role === "admin" ? `Level of ${u.username} once no longer a server admin` : `Level of ${u.username}`}
                    value={u.exec_ceiling_stored}
                    max="host"
                    disabled={self}
                    grant={{ who: u.username, where: "for every workspace they are in" }}
                    onSave={(level) => void run(() => pageApi.setCeiling(u.user_id, level))}
                  />
                  {u.role === "admin" && (
                    <div className="muted small">Host while a server admin; this applies after.</div>
                  )}
                </td>
                <td>
                  {u.disabled ? "disabled" : u.must_change_password ? "temporary password" : "active"}
                </td>
                <td>
                  {!self && (
                    <div className="row end">
                      <button
                        onClick={() => {
                          if (!confirm(`Reset ${u.username}'s password? They are signed out everywhere.`)) return;
                          void run(async () => {
                            const r = await pageApi.resetPassword(u.user_id);
                            setShown({ username: r.username, password: r.password });
                          });
                        }}
                      >
                        Reset password
                      </button>
                      <button onClick={() => run(() => pageApi.updateUser(u.user_id, { disabled: !u.disabled }))}>
                        {u.disabled ? "Enable" : "Disable"}
                      </button>
                      <button
                        className="danger"
                        onClick={() => {
                          if (confirm(`Delete ${u.username}? Their chats stay on disk but become inaccessible.`))
                            void run(() => pageApi.deleteUser(u.user_id));
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
          <select aria-label="Server role" value={role} onChange={(e) => setRole(e.target.value as Role)}>
            <option value="member">User</option>
            <option value="admin">Server admin</option>
          </select>
          <button className="primary" disabled={!username.trim()}>
            Create
          </button>
        </div>
        <span className="field-hint">New accounts start at level none: set it in the list, where host asks first.</span>
      </form>
    </section>
  );
}

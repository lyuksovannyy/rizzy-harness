// Settings → Server → Workspaces (server admins): whether members may create
// workspaces, and every workspace with its owners and members. A team
// workspace's members open below it, to make one of them its owner (when
// the owners are gone or locked out) or to delete it. Server admins see who
// is in a workspace, never its chats.
import { useCallback, useEffect, useState } from "react";
import { pageApi } from "../../apiPages";
import { t } from "../../lib/i18n";
import { workspaceName } from "../../lib/workspaces";
import type { AdminWorkspace, AdminWorkspaceMember, Me } from "../../types";
import { message } from "./panel";

/** One workspace's members, with the owner recovery. */
function Members(props: { me: Me; w: AdminWorkspace; onChanged: () => void }) {
  const { w } = props;
  const [members, setMembers] = useState<AdminWorkspaceMember[] | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const load = useCallback(() => {
    pageApi
      .adminWorkspaceMembers(w.workspace_id)
      .then((r) => setMembers(r.members))
      .catch((x) => setErr(message(x)));
  }, [w.workspace_id]);
  useEffect(load, [load]);
  const owner = async (m: AdminWorkspaceMember) => {
    if (!confirm(`Make ${m.username} an owner of ${w.name}?`)) return;
    setBusy(true);
    setErr(null);
    try {
      await pageApi.setWorkspaceOwner(w.workspace_id, m.user_id);
      load();
      props.onChanged();
    } catch (x) {
      setErr(message(x));
    } finally {
      setBusy(false);
    }
  };
  if (!members) return err ? <div className="error-text">{err}</div> : <div className="muted small">Loading…</div>;
  return (
    <div className="stack ws-admin-members">
      <ul className="ws-members">
        {members.map((m) => {
          const self = m.user_id === props.me.user_id;
          // The server refuses the caller themselves and disabled accounts.
          const why = m.role === "owner" ? null : self ? "You cannot make yourself an owner" : m.disabled ? "Disabled accounts cannot be owners" : null;
          return (
            <li key={m.user_id} className={m.disabled ? "muted" : ""}>
              <span className="grow ellipsis">
                {m.username}
                {self && <span className="muted"> (you)</span>}
                {m.disabled && <span className="muted small"> · disabled</span>}
              </span>
              <span className="muted small">{t(`role.${m.role}`)}</span>
              {m.role !== "owner" && (
                <button
                  type="button"
                  className="small-button"
                  aria-label={`Make ${m.username} an owner`}
                  disabled={busy || !!why}
                  title={why ?? undefined}
                  onClick={() => void owner(m)}
                >
                  Make owner
                </button>
              )}
            </li>
          );
        })}
      </ul>
      {err && <div className="error-text">{err}</div>}
    </div>
  );
}

export function ServerWorkspaces(props: { me: Me; onChanged?: () => void }) {
  const [list, setList] = useState<AdminWorkspace[] | null>(null);
  const [policy, setPolicy] = useState<boolean | null>(null);
  const [open, setOpen] = useState<string | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const changed = props.onChanged ?? (() => {});

  const load = useCallback(() => {
    pageApi
      .adminWorkspaces()
      .then((r) => {
        setList(r.workspaces);
        setPolicy(r.members_can_create);
      })
      .catch((x) => setErr(message(x)));
  }, []);
  useEffect(load, [load]);

  const run = async (f: () => Promise<void>) => {
    setBusy(true);
    setErr(null);
    try {
      await f();
      changed();
    } catch (x) {
      setErr(message(x));
    } finally {
      setBusy(false);
    }
  };
  const remove = (w: AdminWorkspace) => {
    if (!confirm(`Delete ${w.name}? Its members' chats go back to their Personal workspaces; none is deleted.`)) return;
    void run(async () => {
      await pageApi.adminDeleteWorkspace(w.workspace_id);
      if (open === w.workspace_id) setOpen(null);
      load();
    });
  };

  if (!list) return err ? <div className="error-text">{err}</div> : <div className="muted">Loading…</div>;
  return (
    <section className="stack">
      <div className="card stack">
        <label className="check">
          <input
            type="checkbox"
            checked={!!policy}
            disabled={busy}
            onChange={(e) => {
              const on = e.target.checked;
              void run(async () => setPolicy((await pageApi.saveWorkspacePolicy(on)).members_can_create));
            }}
          />
          Members can create workspaces
        </label>
        <span className="field-hint">Server admins always can. Whoever creates a workspace becomes its owner.</span>
      </div>
      {err && <div className="error-text">{err}</div>}
      <table className="users">
        <thead>
          <tr>
            <th>Workspace</th>
            <th>Owners</th>
            <th>Members</th>
            <th />
          </tr>
        </thead>
        <tbody>
          {list.map((w) => {
            const shown = open === w.workspace_id;
            const name = workspaceName(w);
            return [
              <tr key={w.workspace_id}>
                <td>{name}</td>
                <td>{w.owners.length ? w.owners.join(", ") : <span className="error-text">none</span>}</td>
                <td>{w.members}</td>
                <td>
                  {/* A Personal workspace has only its owner: nothing to recover or delete. */}
                  {!w.personal && (
                    <div className="row end">
                      <button
                        type="button"
                        aria-expanded={shown}
                        aria-label={`Members of ${name}`}
                        onClick={() => setOpen(shown ? null : w.workspace_id)}
                      >
                        Members
                      </button>
                      <button type="button" className="danger" aria-label={`Delete ${name}`} disabled={busy} onClick={() => remove(w)}>
                        Delete
                      </button>
                    </div>
                  )}
                </td>
              </tr>,
              shown && (
                <tr key={`${w.workspace_id}:members`}>
                  <td colSpan={4}>
                    <Members
                      me={props.me}
                      w={w}
                      onChanged={() => {
                        load();
                        changed();
                      }}
                    />
                  </td>
                </tr>
              ),
            ];
          })}
        </tbody>
      </table>
    </section>
  );
}

// Settings → Workspace admin → Members: who is in the workspace, their roles
// and levels, adding someone by username, changing a role or a level,
// removing someone, and what each role may do. Choices the server's guards
// would refuse are off (only owners make or touch owners, nobody acts above
// their own role or on their own row, nobody grants a level above their own,
// only owners grant host); the server still checks, and its refusals show
// here.
import { Check } from "lucide-react";
import { type FormEvent, useCallback, useEffect, useState } from "react";
import { pageApi } from "../../apiPages";
import { t } from "../../lib/i18n";
import { canGive, canManage, PERM_LABEL, PERMS, ROLES, rolePerms } from "../../lib/workspaceAdmin";
import type { ExecLevel, WorkspaceMember, WorkspaceRole } from "../../types";
import { atMost, LEVEL_LABEL, LevelSelect } from "../LevelSelect";
import { message, type WorkspacePanelProps } from "./panel";

/** The highest level someone may grant here: their own effective level,
 * and below host unless they are an owner (N.10a part 2). */
function grantable(actor: WorkspaceRole, mine: ExecLevel): ExecLevel {
  if (actor === "owner") return mine;
  return atMost(mine, "sandbox") ? mine : "sandbox";
}

/** The roles as options, each off unless `actor` may give it. */
function RoleOptions({ actor }: { actor: WorkspaceRole }) {
  return (
    <>
      {ROLES.map((r) => (
        <option key={r} value={r} disabled={!canGive(actor, r)}>
          {t(`role.${r}`)}
        </option>
      ))}
    </>
  );
}

export function WorkspaceMembers(props: WorkspacePanelProps) {
  const { workspace: ws, me } = props;
  const id = ws.workspace_id;
  const actor = ws.role;
  const maxLevel = grantable(actor, ws.exec_level);
  const [members, setMembers] = useState<WorkspaceMember[] | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [username, setUsername] = useState("");
  const [role, setRole] = useState<WorkspaceRole>("member");
  const [busy, setBusy] = useState(false);

  const load = useCallback(() => {
    pageApi
      .workspaceMembers(id)
      .then((r) => setMembers(r.members))
      .catch((x) => setErr(message(x)));
  }, [id]);
  // Again when someone joined or left, here or by another admin.
  useEffect(load, [load, ws.members]);

  const run = async (f: () => Promise<unknown>) => {
    setBusy(true);
    setErr(null);
    try {
      await f();
      load();
      props.onChanged();
      return true;
    } catch (x) {
      setErr(message(x));
      return false;
    } finally {
      setBusy(false);
    }
  };
  const add = async (e: FormEvent) => {
    e.preventDefault();
    if (await run(() => pageApi.addWorkspaceMember(id, username.trim(), role))) {
      setUsername("");
      setRole("member");
    }
  };
  const remove = (m: WorkspaceMember) => {
    const who = m.display_name || m.username;
    if (confirm(`Remove ${who} from ${ws.name}? Their chats here go back to their Personal workspace.`))
      void run(() => pageApi.removeWorkspaceMember(id, m.user_id));
  };

  return (
    <section className="stack">
      <p className="muted">
        Members see this workspace in their switcher and start chats in it. Chats stay private to whoever starts them,
        admins and owners included.
      </p>
      {err && <div className="error-text">{err}</div>}
      {members === null ? (
        !err && <div className="muted">Loading…</div>
      ) : (
        <table className="users">
          <thead>
            <tr>
              <th>Member</th>
              <th>Role</th>
              <th>Level</th>
              <th />
            </tr>
          </thead>
          <tbody>
            {members.map((m) => {
              const self = m.user_id === me.user_id;
              const may = canManage(actor, m.role, self);
              const who = m.display_name || m.username;
              return (
                <tr key={m.user_id}>
                  <td>
                    {who}
                    {m.display_name && <span className="muted small"> @{m.username}</span>}
                    {self && <span className="muted"> (you)</span>}
                  </td>
                  <td>
                    <select
                      aria-label={`Role of ${who}`}
                      value={m.role}
                      disabled={!may || busy}
                      onChange={(e) => void run(() => pageApi.setWorkspaceMemberRole(id, m.user_id, e.target.value as WorkspaceRole))}
                    >
                      <RoleOptions actor={actor} />
                    </select>
                  </td>
                  <td>
                    {m.role === "owner" ? (
                      <span title="An owner's level here is always host; their account's level still applies">
                        {LEVEL_LABEL.host}
                      </span>
                    ) : (
                      <LevelSelect
                        label={`Level of ${who}`}
                        value={m.exec_level}
                        max={maxLevel}
                        disabled={!may || busy}
                        grant={{ who, where: `in ${ws.name}` }}
                        onSave={(level) => void run(() => pageApi.setWorkspaceMemberLevel(id, m.user_id, level))}
                      />
                    )}
                  </td>
                  <td>
                    {may && (
                      <div className="row end">
                        <button type="button" className="danger" aria-label={`Remove ${who}`} disabled={busy} onClick={() => remove(m)}>
                          Remove
                        </button>
                      </div>
                    )}
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      )}

      <form className="card" onSubmit={(e) => void add(e)}>
        <h3>Add a member</h3>
        <div className="row">
          <input
            placeholder="username"
            aria-label="Username"
            autoComplete="off"
            value={username}
            onChange={(e) => setUsername(e.target.value)}
          />
          <select aria-label="Role of the new member" value={role} onChange={(e) => setRole(e.target.value as WorkspaceRole)}>
            <RoleOptions actor={actor} />
          </select>
          <button className="primary" disabled={busy || !username.trim() || !canGive(actor, role)}>
            Add
          </button>
        </div>
        <span className="field-hint">They need an account on this server; a server admin creates those.</span>
      </form>

      <div className="card stack">
        <h3>Roles</h3>
        <table className="users ws-roles">
          <thead>
            <tr>
              <th>Role</th>
              {PERMS.map((p) => (
                <th key={p}>{PERM_LABEL[p]}</th>
              ))}
            </tr>
          </thead>
          <tbody>
            {ROLES.map((r) => (
              <tr key={r}>
                <td>{t(`role.${r}`)}</td>
                {PERMS.map((p) => (
                  <td key={p}>
                    {rolePerms(r).includes(p) ? (
                      <Check size={16} aria-label="yes" />
                    ) : (
                      <span className="muted" aria-label="no">
                        —
                      </span>
                    )}
                  </td>
                ))}
              </tr>
            ))}
          </tbody>
        </table>
        <span className="field-hint">
          Only owners delete the workspace and make or change owners. Admins manage members up to their own role.
          Nobody changes their own role, and the last owner cannot leave or step down while others remain (a server
          admin can name a new owner if every owner's account is gone). Which agent chats a member may start
          is their level, not their role: none (every new member), container (not offered yet), sandbox, or
          host, a shell on the node with no sandbox, which only owners grant. Nobody grants above their own
          level or changes their own, and an account's level, set by a server admin, still caps it.
        </span>
      </div>
    </section>
  );
}

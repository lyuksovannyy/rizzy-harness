// Settings → Workspace → Overview, for every member: what the workspace is,
// the viewer's role and what it allows, who is in it, the defaults new chats
// start from, and the two choices that are the member's own in a team
// workspace: whether their memory is used there, and leaving.
import { useEffect, useState } from "react";
import { pageApi } from "../../apiPages";
import { t } from "../../lib/i18n";
import { modelLabel } from "../../lib/models";
import { roleSummary } from "../../lib/workspaceAdmin";
import { workspaceName } from "../../lib/workspaces";
import type { WorkspaceDetail, WorkspaceMember } from "../../types";
import { message, type WorkspacePanelProps } from "./panel";

type Where = "load" | "memory" | "leave";

export function WorkspaceOverview(props: WorkspacePanelProps) {
  const { workspace: ws, me } = props;
  const id = ws.workspace_id;
  const [detail, setDetail] = useState<WorkspaceDetail | null>(null);
  const [members, setMembers] = useState<WorkspaceMember[] | null>(null);
  const [err, setErr] = useState<{ at: Where; text: string } | null>(null);
  const [busy, setBusy] = useState(false);
  // Again when the member count or the viewer's role changes (someone joined,
  // left or was moved, here or by another admin).
  useEffect(() => {
    const fail = (x: unknown) => setErr({ at: "load", text: message(x) });
    pageApi.workspace(id).then(setDetail).catch(fail);
    // Personal has only its owner.
    if (!ws.personal) pageApi.workspaceMembers(id).then((r) => setMembers(r.members)).catch(fail);
  }, [id, ws.personal, ws.members, ws.role]);

  const run = async (at: Where, f: () => Promise<void>) => {
    setBusy(true);
    setErr(null);
    try {
      await f();
    } catch (x) {
      setErr({ at, text: message(x) });
    } finally {
      setBusy(false);
    }
  };
  const errorAt = (at: Where) => err?.at === at && <div className="error-text">{err.text}</div>;
  // The server deletes a team workspace when its last member leaves.
  const alone = (members?.length ?? ws.members) <= 1;
  const leave = () => {
    const self = me.user_id;
    const what = alone
      ? `Leave ${ws.name}? You are its only member, so leaving deletes it with its settings.`
      : `Leave ${ws.name}?`;
    if (!self || !confirm(`${what} Your chats here go back to your Personal workspace.`)) return;
    void run("leave", async () => {
      await pageApi.removeWorkspaceMember(id, self);
      props.onLeft();
    });
  };

  const agent = props.agents.find((a) => a.id === props.defaultAgent);
  const model = props.models.find((m) => m.default);
  // Where each default comes from, once the workspace's own settings are known.
  const from = (own: boolean) => (detail ? (own ? "set for this workspace" : "the server's") : "");

  return (
    <div className="stack">
      <div className="card stack">
        <h3>{workspaceName(ws)}</h3>
        {ws.personal ? (
          <p className="muted">Your own workspace: only you are in it.</p>
        ) : (
          <>
            <div className="ws-role">
              <span className="ws-role-name">{t(`role.${ws.role}`)}</span>
              <span className="muted">{roleSummary(ws.role)}</span>
            </div>
            <p className="muted small">
              Chats stay private to whoever starts them: the workspace's admins and owners do not see yours.
            </p>
          </>
        )}
        {errorAt("load")}
      </div>

      <div className="card stack">
        <h3>New chats</h3>
        <dl className="ws-facts">
          <div>
            <dt>Default agent</dt>
            <dd>
              {agent?.name ?? props.defaultAgent} <span className="muted small">{from(!!detail?.settings.default_agent)}</span>
            </dd>
          </div>
          <div>
            <dt>Default model</dt>
            <dd>
              {model ? modelLabel(model, props.models) : "none yet"}{" "}
              <span className="muted small">{from(!!detail?.settings.agent_models.general)}</span>
            </dd>
          </div>
        </dl>
        <span className="field-hint">Defaults you set in Preferences come first, in every workspace.</span>
      </div>

      {!ws.personal && (
        <div className="card stack">
          <h3>Members</h3>
          {members === null ? (
            <div className="muted">Loading…</div>
          ) : (
            <ul className="ws-members">
              {members.map((m) => (
                <li key={m.user_id}>
                  <span className="grow ellipsis">
                    {m.display_name || m.username}
                    {m.display_name && <span className="muted small"> @{m.username}</span>}
                    {m.user_id === me.user_id && <span className="muted"> (you)</span>}
                  </span>
                  <span className="muted small">{t(`role.${m.role}`)}</span>
                </li>
              ))}
            </ul>
          )}
        </div>
      )}

      {!ws.personal && (
        <div className="card stack">
          <h3>Memory</h3>
          <label className="check">
            <input
              type="checkbox"
              checked={detail?.use_memory ?? true}
              disabled={!detail || busy}
              onChange={(e) => {
                const on = e.target.checked;
                void run("memory", async () => setDetail(await pageApi.setWorkspaceMemory(id, on)));
              }}
            />
            Use my memory in this workspace
          </label>
          <span className="field-hint">
            On, your chats here recall what your memory holds and can propose new facts for it. Off, they neither read
            nor add to it. Only you choose this; nobody else here sees your memory.
          </span>
          {errorAt("memory")}
        </div>
      )}

      {!ws.personal && (
        <div className="card stack">
          <h3>Leave</h3>
          <p className="muted">
            {alone
              ? "You are its only member: leaving deletes the workspace and its settings. Your chats here go back to your Personal workspace."
              : "Your chats here go back to your Personal workspace; nothing is deleted. To come back, an admin has to add you again."}
          </p>
          {errorAt("leave")}
          <div>
            <button type="button" className="danger" disabled={busy} onClick={leave}>
              Leave {ws.name}
            </button>
          </div>
        </div>
      )}
    </div>
  );
}

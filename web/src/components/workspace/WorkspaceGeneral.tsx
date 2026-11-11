// Settings → Workspace admin → General: the workspace's name and the agent
// its new chats start with. Each is sent only when it changed, and a new
// default agent goes into the settings as the server has them just before
// the save, so this panel changes only that field (a chain saved between
// that read and this save can still be lost).
import { type FormEvent, useEffect, useState } from "react";
import { pageApi } from "../../apiPages";
import type { WorkspaceDetail } from "../../types";
import { AgentMark, agentAlert } from "../AgentAvatar";
import { Select } from "../Select";
import { message, type WorkspacePanelProps } from "./panel";

export function WorkspaceGeneral(props: WorkspacePanelProps) {
  const id = props.workspace.workspace_id;
  const [detail, setDetail] = useState<WorkspaceDetail | null>(null);
  const [name, setName] = useState("");
  const [agent, setAgent] = useState("");
  const [msg, setMsg] = useState<string | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const show = (d: WorkspaceDetail) => {
    setDetail(d);
    setName(d.name);
    setAgent(d.settings.default_agent ?? "");
  };
  useEffect(() => {
    pageApi
      .workspace(id)
      .then(show)
      .catch((x) => setErr(message(x)));
  }, [id]);

  if (!detail) return err ? <div className="error-text">{err}</div> : <div className="muted">Loading…</div>;
  const trimmed = name.trim();
  const renamed = trimmed !== detail.name;
  const reagent = (agent || null) !== (detail.settings.default_agent ?? null);
  const save = async (e: FormEvent) => {
    e.preventDefault();
    setBusy(true);
    setErr(null);
    setMsg(null);
    try {
      // The whole document goes back: read again, not as this panel loaded it.
      const fresh = reagent ? (await pageApi.workspace(id)).settings : null;
      show(
        await pageApi.updateWorkspace(id, {
          ...(renamed ? { name: trimmed } : {}),
          ...(fresh ? { settings: { ...fresh, default_agent: agent || null } } : {}),
        }),
      );
      setMsg("Saved.");
      props.onChanged();
    } catch (x) {
      setErr(message(x));
    } finally {
      setBusy(false);
    }
  };

  return (
    <form className="card stack form" onSubmit={(e) => void save(e)}>
      <label>
        Name
        <input
          value={name}
          maxLength={64}
          onChange={(e) => {
            setName(e.target.value);
            setMsg(null);
          }}
        />
      </label>
      <div className="field">
        Default agent
        <Select
          label="Default agent"
          value={agent}
          options={[
            { value: "", label: "Server default" },
            ...props.agents.map((a) => ({
              value: a.id,
              label: a.name,
              hint: agentAlert(a) ? "model unavailable" : undefined,
              icon: <AgentMark agent={a} size={18} />,
            })),
          ]}
          onChange={(v) => {
            setAgent(v);
            setMsg(null);
          }}
        />
        <span className="field-hint">
          New chats here start with it, unless a member picked a default agent in their Preferences.
        </span>
      </div>
      {err && <div className="error-text">{err}</div>}
      <div className="row end">
        {msg && <span className="muted">{msg}</span>}
        <button className="primary" disabled={busy || !trimmed || (!renamed && !reagent)}>
          Save
        </button>
      </div>
    </form>
  );
}

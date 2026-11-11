// Settings → Workspace admin → Delete (owners): typing the workspace's name
// unlocks it. Chats are never deleted with it: each member's go back to
// their Personal workspace. Then the app goes home, in Personal.
import { useState } from "react";
import { pageApi } from "../../apiPages";
import { message, type WorkspacePanelProps } from "./panel";

export function WorkspaceDelete(props: WorkspacePanelProps) {
  const ws = props.workspace;
  const [typed, setTyped] = useState("");
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const others = ws.members - 1;
  const remove = async () => {
    setBusy(true);
    setErr(null);
    try {
      await pageApi.deleteWorkspace(ws.workspace_id);
      props.onLeft();
    } catch (x) {
      setErr(message(x));
      setBusy(false);
    }
  };
  return (
    <div className="card stack form">
      <h3>Delete {ws.name}</h3>
      <p className="muted">
        {others > 0
          ? `It goes for you and ${others === 1 ? "1 other member" : `${others} other members`}, with its settings. `
          : "It goes with its settings. "}
        No chat is deleted: everyone's chats from here go back to their own Personal workspace. This cannot be undone.
      </p>
      <label>
        <span>
          Type <strong>{ws.name}</strong> to confirm
        </span>
        <input value={typed} autoComplete="off" onChange={(e) => setTyped(e.target.value)} />
      </label>
      {err && <div className="error-text">{err}</div>}
      <div className="row end">
        <button type="button" className="danger" disabled={busy || typed.trim() !== ws.name} onClick={() => void remove()}>
          Delete workspace
        </button>
      </div>
    </div>
  );
}

import { useState } from "react";
import { api } from "../api";

/** Self-service password change; `forced` after an admin-set temporary password. */
export function ChangePassword(props: { forced: boolean; onDone: () => void; onCancel?: () => void }) {
  const [current, setCurrent] = useState("");
  const [next, setNext] = useState("");
  const [again, setAgain] = useState("");
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (next !== again) {
      setErr("the new passwords do not match");
      return;
    }
    setBusy(true);
    setErr(null);
    try {
      await api.changePassword(current, next);
      props.onDone();
    } catch (x) {
      setErr(x instanceof Error ? x.message : String(x));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className={props.forced ? "login" : "panel"}>
      <form className="card narrow" onSubmit={submit}>
        <h2>{props.forced ? "Choose your password" : "Change password"}</h2>
        {props.forced && (
          <p className="muted">Your account was created with a temporary password. Pick your own to continue.</p>
        )}
        <input
          type="password"
          autoFocus
          placeholder={props.forced ? "temporary password" : "current password"}
          autoComplete="current-password"
          value={current}
          onChange={(e) => setCurrent(e.target.value)}
        />
        <input
          type="password"
          placeholder="new password (10+ characters)"
          autoComplete="new-password"
          value={next}
          onChange={(e) => setNext(e.target.value)}
        />
        <input
          type="password"
          placeholder="new password again"
          autoComplete="new-password"
          value={again}
          onChange={(e) => setAgain(e.target.value)}
        />
        {err && <div className="error-text">{err}</div>}
        <div className="row end">
          {props.onCancel && (
            <button type="button" onClick={props.onCancel}>
              Cancel
            </button>
          )}
          <button className="primary" disabled={busy || !current || !next || !again}>
            Save password
          </button>
        </div>
      </form>
    </div>
  );
}

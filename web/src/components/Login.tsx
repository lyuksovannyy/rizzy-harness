import { useState } from "react";
import { api } from "../api";

export function Login({ onDone }: { onDone: () => void }) {
  const [token, setToken] = useState("");
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    setBusy(true);
    setErr(null);
    try {
      await api.login(token.trim());
      onDone();
    } catch (x) {
      setErr(x instanceof Error ? x.message : String(x));
    } finally {
      setBusy(false);
    }
  };
  return (
    <div className="login">
      <form onSubmit={submit} className="card">
        <h1>harness</h1>
        <p className="muted">
          Paste the token from <code>ui-token</code> in the data directory, or run{" "}
          <code>harness token</code>.
        </p>
        <input
          type="password"
          autoFocus
          autoComplete="current-password"
          placeholder="token"
          value={token}
          onChange={(e) => setToken(e.target.value)}
        />
        {err && <div className="error-text">{err}</div>}
        <button className="primary" disabled={busy || !token.trim()}>
          Log in
        </button>
      </form>
    </div>
  );
}

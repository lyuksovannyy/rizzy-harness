import { APP_TITLE } from "../lib/brand";
import { useEffect, useState } from "react";
import { api } from "../api";

/** Login, or first-run claim when the server has no owner yet. */
export function Login({ onDone }: { onDone: () => void }) {
  const [claimed, setClaimed] = useState<boolean | null>(null);
  const [code, setCode] = useState("");
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    api
      .setupStatus()
      .then((r) => setClaimed(r.claimed))
      .catch(() => setClaimed(true));
  }, []);

  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    setBusy(true);
    setErr(null);
    try {
      if (claimed) await api.login(username.trim(), password);
      else await api.setup(code.trim(), username.trim().toLowerCase(), password);
      onDone();
    } catch (x) {
      setErr(x instanceof Error ? x.message : String(x));
    } finally {
      setBusy(false);
    }
  };

  if (claimed === null) return <div className="splash">Loading…</div>;

  return (
    <div className="login">
      <form onSubmit={submit} className="card">
        <h1>{APP_TITLE}</h1>
        {claimed ? (
          <p className="muted">Log in to continue.</p>
        ) : (
          <>
            <p className="muted">
              First run. Enter the claim code from <code>claim-code</code> in the data directory
              (or run <code>harness claim-code</code>), then create your account.
            </p>
            <input
              autoFocus
              className="mono"
              placeholder="claim code"
              autoComplete="off"
              value={code}
              onChange={(e) => setCode(e.target.value)}
            />
          </>
        )}
        <input
          autoFocus={claimed}
          placeholder="username"
          autoComplete="username"
          value={username}
          onChange={(e) => setUsername(e.target.value)}
        />
        <input
          type="password"
          placeholder={claimed ? "password" : "password (10+ characters)"}
          autoComplete={claimed ? "current-password" : "new-password"}
          value={password}
          onChange={(e) => setPassword(e.target.value)}
        />
        {err && <div className="error-text">{err}</div>}
        <button
          className="primary"
          disabled={busy || !username.trim() || !password || (!claimed && !code.trim())}
        >
          {claimed ? "Log in" : "Create account"}
        </button>
      </form>
    </div>
  );
}

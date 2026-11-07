import { APP_TITLE } from "../lib/brand";
import { useEffect, useRef, useState } from "react";
import { api } from "../api";
import { useCopy } from "../md/copy";
import { Eye, EyeOff } from "lucide-react";
import { MIN_PASSWORD, passwordStrength } from "../lib/password";

const CLAIM_COMMAND = "harness claim-code";

/** A claim code is 20 characters in 4 groups; the server ignores case,
 * dashes and spaces, so only the characters count here. */
const claimComplete = (code: string) => code.replace(/[^a-z0-9]/gi, "").length >= 20;

/** Login, or first-run claim when the server has no owner yet. */
export function Login({ onDone }: { onDone: () => void }) {
  const [claimed, setClaimed] = useState<boolean | null>(null);
  const [code, setCode] = useState("");
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [again, setAgain] = useState("");
  const [reveal, setReveal] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [copied, copy] = useCopy();
  const userRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    api
      .setupStatus()
      .then((r) => setClaimed(r.claimed))
      .catch(() => setClaimed(true));
  }, []);

  // The account fields open once the code is whole; move on to them then.
  const unlocked = claimed === true || claimComplete(code);
  useEffect(() => {
    if (claimed === false && unlocked) userRef.current?.focus();
  }, [claimed, unlocked]);

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

  const usernameField = (
    <input
      ref={userRef}
      autoFocus={claimed}
      placeholder="username"
      autoComplete="username"
      value={username}
      onChange={(e) => setUsername(e.target.value)}
    />
  );

  const strength = passwordStrength(password);
  // While the confirmation is still a prefix it is being typed, not wrong.
  const mismatch = again !== "" && !password.startsWith(again);
  const pwType = reveal ? "text" : "password";
  // One toggle shows both new-password fields, so they can be compared.
  const newAccount = (
    <>
      {usernameField}
      <div className="pw-field">
        <input
          type={pwType}
          placeholder={`password (${MIN_PASSWORD}+ characters)`}
          autoComplete="new-password"
          aria-describedby="pw-strength"
          value={password}
          onChange={(e) => setPassword(e.target.value)}
        />
        <button
          type="button"
          className="icon"
          aria-label={reveal ? "Hide password" : "Show password"}
          aria-pressed={reveal}
          onClick={() => setReveal((r) => !r)}
        >
          {reveal ? <EyeOff size={16} aria-hidden /> : <Eye size={16} aria-hidden />}
        </button>
      </div>
      {password && (
        <div className="pw-strength" data-score={strength.score} id="pw-strength">
          <div className="pw-bars" aria-hidden>
            {[1, 2, 3, 4].map((i) => (
              <span key={i} className={i <= strength.score ? "on" : undefined} />
            ))}
          </div>
          <span className="pw-label">{strength.label}</span>
        </div>
      )}
      <input
        type={pwType}
        placeholder="confirm password"
        autoComplete="new-password"
        aria-invalid={mismatch}
        value={again}
        onChange={(e) => setAgain(e.target.value)}
      />
      {mismatch && <div className="error-text">Passwords don't match.</div>}
    </>
  );
  const setupReady =
    unlocked && !!username.trim() && strength.score > 0 && again === password;

  return (
    <div className="login">
      <form onSubmit={submit} className="card">
        {claimed ? (
          <>
            <h1>{APP_TITLE}</h1>
            <p className="muted">Log in to continue.</p>
            {usernameField}
            <input
              type="password"
              placeholder="password"
              autoComplete="current-password"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
            />
          </>
        ) : (
          <>
            <header className="setup-head">
              <span className="eyebrow">{APP_TITLE}</span>
              <h1>Setup</h1>
              <p className="muted">
                Just making sure you're the person who installed this server.
              </p>
            </header>
            <ol className="setup-steps">
              <li>
                <span>On the server, run:</span>
                <div className="setup-cmd">
                  <code>{CLAIM_COMMAND}</code>
                  <button type="button" className="copy" onClick={() => copy(CLAIM_COMMAND)}>
                    {copied ? "Copied" : "Copy"}
                  </button>
                </div>
                <span className="setup-alt">
                  or open <code>claim-code</code> in the data directory.
                </span>
              </li>
              <li>
                <span>Enter the code it prints:</span>
                <input
                  autoFocus
                  className="mono"
                  placeholder="XXXXX-XXXXX-XXXXX-XXXXX"
                  autoComplete="off"
                  spellCheck={false}
                  aria-label="claim code"
                  value={code}
                  onChange={(e) => setCode(e.target.value)}
                />
              </li>
              {unlocked && (
                <li className="setup-account">
                  <span>Create your account:</span>
                  {newAccount}
                </li>
              )}
            </ol>
          </>
        )}
        {err && <div className="error-text">{err}</div>}
        <button
          className="primary"
          disabled={busy || (claimed ? !username.trim() || !password : !setupReady)}
        >
          {claimed ? "Log in" : "Create account"}
        </button>
      </form>
    </div>
  );
}

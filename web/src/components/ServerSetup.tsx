// Settings → Nodes → Set up a server over SSH. The password or private key
// is sent once, used to install harness's own key, and not kept.
import { useEffect, useState } from "react";
import { api } from "../api";
import type { SetupState, SetupStep } from "../types";

function Step({ s, jobId, onState }: { s: SetupStep; jobId: string; onState: (v: SetupState) => void }) {
  const [copied, setCopied] = useState(false);
  const label: Record<SetupStep["status"], string> = {
    pending: "…",
    running: "running",
    ok: "done",
    failed: "failed",
    todo: "to do",
    waiting: "your turn",
    skipped: "skipped",
  };
  return (
    <li className={`setup-step st-${s.status}`}>
      <div className="row">
        <b>{s.title}</b>
        <span className="pill small">{label[s.status]}</span>
      </div>
      {s.detail && <div className="setup-detail small">{s.detail}</div>}
      {s.script && (
        <details open={s.status === "todo"}>
          <summary className="small">
            {s.status === "todo" ? "Commands to run" : "What runs"}
            {s.root ? " (as root: sudo bash)" : ""}
          </summary>
          <pre className="mono join-cmd">{s.script}</pre>
          <button
            type="button"
            className="link small"
            onClick={() => {
              void navigator.clipboard?.writeText(s.script!).then(() => setCopied(true));
            }}
          >
            {copied ? "Copied" : "Copy"}
          </button>
        </details>
      )}
      {s.status === "waiting" && (
        <button type="button" className="primary" onClick={() => void api.setupContinue(jobId).then(() => api.setupState(jobId).then(onState))}>
          I ran it, continue
        </button>
      )}
    </li>
  );
}

export function ServerSetup({ onDone }: { onDone: () => void }) {
  const [host, setHost] = useState("");
  const [port, setPort] = useState(22);
  const [user, setUser] = useState("root");
  const [fp, setFp] = useState<string | null>(null);
  const [fpOk, setFpOk] = useState(false);
  const [useKey, setUseKey] = useState(false);
  const [password, setPassword] = useState("");
  const [privateKey, setPrivateKey] = useState("");
  const [passphrase, setPassphrase] = useState("");
  const [mode, setMode] = useState<"auto" | "manual">("auto");
  const [ownKey, setOwnKey] = useState("");
  const [copyProvider, setCopyProvider] = useState(false);
  const [job, setJob] = useState<SetupState | null>(null);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  useEffect(() => {
    if (!job || job.finished) {
      if (job?.finished && job.node_id) onDone();
      return;
    }
    const t = setTimeout(() => void api.setupState(job.id).then(setJob).catch((x) => setErr(String(x))), 1000);
    return () => clearTimeout(t);
  }, [job, onDone]);

  if (job) {
    return (
      <div className="stack">
        <div className="muted small">
          {job.mode === "auto" ? "Automatic setup" : "Manual setup"} of {job.host}
          {job.finished && job.error && <span className="error-text"> · stopped: {job.error}</span>}
          {job.finished && job.node_id && <span> · node `{job.node_id}` is online</span>}
        </div>
        <ol className="setup-steps">
          {job.steps.map((s) => (
            <Step key={s.id} s={s} jobId={job.id} onState={setJob} />
          ))}
        </ol>
        <div className="row">
          {job.mode === "manual" && job.finished && !job.error && (
            <button
              type="button"
              disabled={busy}
              onClick={async () => {
                setBusy(true);
                setErr(null);
                try {
                  setJob(await api.setupRecheck(job.id));
                } catch (x) {
                  setErr(x instanceof Error ? x.message : String(x));
                } finally {
                  setBusy(false);
                }
              }}
            >
              {busy ? "Checking…" : "Check again"}
            </button>
          )}
          <button type="button" className="link" onClick={() => setJob(null)}>
            Set up another server
          </button>
        </div>
        {err && <div className="error-text">{err}</div>}
      </div>
    );
  }

  return (
    <form
      className="form stack"
      onSubmit={async (e) => {
        e.preventDefault();
        setErr(null);
        setBusy(true);
        try {
          const { id } = await api.startSetup({
            host: host.trim(),
            port,
            user: user.trim(),
            fingerprint: fp,
            mode,
            ...(useKey ? { private_key: privateKey, passphrase: passphrase || undefined } : { password }),
            own_pubkey: mode === "auto" && ownKey.trim() ? ownKey.trim() : undefined,
            copy_provider: mode === "auto" && copyProvider,
          });
          // Nothing secret stays in the page once it is sent.
          setPassword("");
          setPrivateKey("");
          setPassphrase("");
          setJob(await api.setupState(id));
        } catch (x) {
          setErr(x instanceof Error ? x.message : String(x));
        } finally {
          setBusy(false);
        }
      }}
    >
      <div className="row">
        <label className="grow">
          Server address
          <input className="mono" placeholder="203.0.113.7" value={host} onChange={(e) => { setHost(e.target.value); setFp(null); setFpOk(false); }} />
        </label>
        <label>
          SSH port
          <input type="number" min={1} max={65535} value={port} onChange={(e) => { setPort(Number(e.target.value)); setFp(null); setFpOk(false); }} />
        </label>
        <label>
          User
          <input className="mono" value={user} onChange={(e) => setUser(e.target.value)} />
        </label>
      </div>
      {!fp ? (
        <div className="row">
          <button
            type="button"
            disabled={!host.trim() || busy}
            onClick={async () => {
              setErr(null);
              setBusy(true);
              try {
                setFp((await api.probeHost(host.trim(), port)).fingerprint);
              } catch (x) {
                setErr(x instanceof Error ? x.message : String(x));
              } finally {
                setBusy(false);
              }
            }}
          >
            {busy ? "Connecting…" : "Check the server's identity"}
          </button>
        </div>
      ) : (
        <>
          <div className="card-inner small">
            Host key <code>{fp}</code>. Compare it with your provider's console (or run <code>ssh-keygen -lf /etc/ssh/ssh_host_ed25519_key.pub</code> there) before
            sending a password. A mismatch means someone may be in between.
            <label className="check">
              <input type="checkbox" checked={fpOk} onChange={(e) => setFpOk(e.target.checked)} /> It matches
            </label>
          </div>
          <div className="engines">
            <label className="check">
              <input type="radio" name="auth" checked={!useKey} onChange={() => setUseKey(false)} /> Password
            </label>
            <label className="check">
              <input type="radio" name="auth" checked={useKey} onChange={() => setUseKey(true)} /> Private key
            </label>
          </div>
          {!useKey ? (
            <label>
              Password (used once to install harness's own key; not stored)
              <input type="password" autoComplete="off" value={password} onChange={(e) => setPassword(e.target.value)} />
            </label>
          ) : (
            <>
              <label>
                Private key (OpenSSH, ed25519 or ECDSA; used once, not stored)
                <textarea className="mono" rows={4} value={privateKey} onChange={(e) => setPrivateKey(e.target.value)} />
              </label>
              <label>
                Passphrase (if any)
                <input type="password" autoComplete="off" value={passphrase} onChange={(e) => setPassphrase(e.target.value)} />
              </label>
            </>
          )}
          <div className="presets">
            <label className="check">
              <input type="radio" name="mode" checked={mode === "auto"} onChange={() => setMode("auto")} />
              <b>Automatic</b> <span className="muted small">for a fresh VPS or one used only as a node: installs WireGuard, a firewall (nftables), key-only SSH and the node</span>
            </label>
            <label className="check">
              <input type="radio" name="mode" checked={mode === "manual"} onChange={() => setMode("manual")} />
              <b>Manual</b> <span className="muted small">for a server with other things on it: checks it and shows the commands to run yourself</span>
            </label>
          </div>
          {mode === "auto" && (
            <>
              <label>
                Your SSH public key (optional, so you can still log in after passwords are turned off)
                <input className="mono" placeholder="ssh-ed25519 AAAA… you@laptop" value={ownKey} onChange={(e) => setOwnKey(e.target.value)} />
              </label>
              <label className="check">
                <input type="checkbox" checked={copyProvider} onChange={(e) => setCopyProvider(e.target.checked)} />
                Copy this server's AI provider and its API key to the node (the node calls the model itself)
              </label>
            </>
          )}
          <div className="row end">
            <button className="primary" disabled={!fpOk || busy || (!useKey && !password) || (useKey && !privateKey.trim())}>
              {busy ? "Starting…" : mode === "auto" ? "Set up automatically" : "Check the server"}
            </button>
          </div>
        </>
      )}
      {err && <div className="error-text">{err}</div>}
    </form>
  );
}

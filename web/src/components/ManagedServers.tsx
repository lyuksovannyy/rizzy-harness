// Servers set up over SSH, after the setup: mode, watchdog, firewall.
import { useCallback, useEffect, useState } from "react";
import { api } from "../api";
import type { SetupCommands, SetupRecord } from "../types";

function Cmd({ label, text }: { label: string; text: string }) {
  return (
    <details>
      <summary className="small">{label}</summary>
      <pre className="mono join-cmd">{text}</pre>
    </details>
  );
}

function Server({ r, cmds, reload }: { r: SetupRecord; cmds: SetupCommands; reload: () => void }) {
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const [rootFor, setRootFor] = useState<boolean | null>(null);
  const [rootKey, setRootKey] = useState("");
  const [rootPw, setRootPw] = useState("");
  const act = async (p: Promise<unknown>) => {
    setBusy(true);
    setErr(null);
    try {
      await p;
      reload();
    } catch (x) {
      setErr(x instanceof Error ? x.message : String(x));
    } finally {
      setBusy(false);
    }
  };
  const firewall = (enable: boolean) => {
    if (!r.admin_sudo && rootFor === null) {
      setRootFor(enable);
      return;
    }
    void act(
      api.setFirewall(r.id, {
        enable,
        ...(rootKey.trim() ? { private_key: rootKey } : rootPw ? { password: rootPw } : {}),
      }),
    ).then(() => {
      setRootFor(null);
      setRootKey("");
      setRootPw("");
    });
  };
  return (
    <li className={`setup-step ${r.problems.length ? "st-todo" : ""}`}>
      <div className="row">
        <b>{r.host}</b>
        <span className="muted small">
          {r.node_id ? `node ${r.node_id}` : "not joined"} · harness logs in as <code>{r.login_user}</code> ·{" "}
          {r.engine === "none" ? "no container engine" : r.engine}
        </span>
      </div>
      {r.problems.map((p, i) => (
        <div key={i} className="error-text small">
          {p.detail}
        </div>
      ))}
      <div className="row">
        <label>
          Mode
          <select value={r.mode} disabled={busy} onChange={(e) => void act(api.updateSetup(r.id, { mode: e.target.value as "auto" | "manual" }))}>
            <option value="auto">Automatic: harness makes changes (with a root login you give per action)</option>
            <option value="manual">Manual: harness only shows the commands</option>
          </select>
        </label>
      </div>
      <div className="presets">
        <label className="check">
          <input type="checkbox" checked={r.watch_ssh} disabled={busy} onChange={(e) => void act(api.updateSetup(r.id, { watch_ssh: e.target.checked }))} />
          Watch SSH: report if password logins come back
        </label>
        <label className="check">
          <input
            type="checkbox"
            checked={r.watch_firewall}
            disabled={busy || !r.firewall_enabled}
            onChange={(e) => void act(api.updateSetup(r.id, { watch_firewall: e.target.checked }))}
          />
          Watch the firewall: report if its rules are gone {r.firewall_enabled ? "" : "(firewall is off)"}
        </label>
      </div>
      <div className="row small">
        <span>Firewall: {r.firewall_enabled ? "on" : "off"}</span>
        {r.mode === "auto" && (
          <button type="button" disabled={busy} onClick={() => firewall(!r.firewall_enabled)}>
            {r.firewall_enabled ? "Turn the firewall off" : "Turn the firewall on"}
          </button>
        )}
        <button type="button" className="link" disabled={busy} onClick={() => void act(api.checkSetup(r.id))}>
          {busy ? "…" : "Check now"}
        </button>
        <span className="muted">{r.last_check_ms ? `checked ${new Date(r.last_check_ms).toLocaleString()}` : "not checked yet"}</span>
      </div>
      {rootFor !== null && (
        <div className="card-inner stack small">
          Harness keeps no root access here. Give a root login for this one action (used once, not stored):
          <textarea className="mono" rows={3} placeholder="root's private key (OpenSSH)" value={rootKey} onChange={(e) => setRootKey(e.target.value)} />
          <input type="password" placeholder="or root's password (only if password logins are on)" value={rootPw} onChange={(e) => setRootPw(e.target.value)} />
          <div className="row">
            <button type="button" className="primary" disabled={busy || (!rootKey.trim() && !rootPw)} onClick={() => firewall(rootFor)}>
              {rootFor ? "Turn on" : "Turn off"}
            </button>
            <button type="button" onClick={() => setRootFor(null)}>
              Cancel
            </button>
          </div>
        </div>
      )}
      {r.mode === "manual" && <Cmd label={r.firewall_enabled ? "Commands: firewall off" : "Commands: firewall on"} text={r.firewall_enabled ? cmds.firewall_off : cmds.firewall_on} />}
      <div className="muted small">
        SSH protection is changed by hand only. Agents run as <code>harness</code> without root, by design, so a chat cannot
        change it; if one could, an injected prompt could turn password logins back on.
      </div>
      <Cmd label="Commands: key-only SSH on" text={cmds.ssh_on} />
      <Cmd label="Commands: allow password logins again" text={cmds.ssh_off} />
      {err && <div className="error-text">{err}</div>}
      <button
        type="button"
        className="link small"
        onClick={() => {
          if (confirm(`Forget ${r.host}? Nothing on the server changes.`)) void act(api.forgetSetup(r.id));
        }}
      >
        Forget this server
      </button>
    </li>
  );
}

export function ManagedServers({ version }: { version: number }) {
  const [list, setList] = useState<{ setup: SetupRecord; commands: SetupCommands }[]>([]);
  const load = useCallback(() => {
    api
      .setups()
      .then((r) => setList(r.setups))
      .catch(() => {});
  }, []);
  useEffect(load, [load, version]);
  if (list.length === 0) return null;
  return (
    <div className="card stack">
      <h3>Servers set up over SSH</h3>
      <ol className="setup-steps managed">
        {list.map((x) => (
          <Server key={x.setup.id} r={x.setup} cmds={x.commands} reload={load} />
        ))}
      </ol>
    </div>
  );
}

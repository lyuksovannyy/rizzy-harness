// Settings, loaded lazily: Preferences, Password, and Users for admins.
import { useEffect, useState } from "react";
import { api } from "../api";
import { t } from "../lib/i18n";
import type { Agent, Me, ModelInfo, Prefs } from "../types";
import { ChangePassword } from "./ChangePassword";
import { Users } from "./Users";

type Tab = "preferences" | "password" | "users";

function Preferences(props: { agents: Agent[]; models: ModelInfo[]; onSaved: (p: Prefs) => void }) {
  const [p, setP] = useState<Prefs | null>(null);
  const [msg, setMsg] = useState<string | null>(null);
  const [err, setErr] = useState<string | null>(null);
  useEffect(() => {
    api.settings().then(setP).catch((e) => setErr(String(e)));
  }, []);
  if (!p) return <div className="muted">{err ?? "Loading…"}</div>;
  const save = async (e: React.FormEvent) => {
    e.preventDefault();
    setErr(null);
    setMsg(null);
    try {
      const saved = await api.saveSettings(p);
      props.onSaved(saved);
      setMsg("Saved.");
    } catch (x) {
      setErr(x instanceof Error ? x.message : String(x));
    }
  };
  return (
    <form className="card form" onSubmit={save}>
      <label>
        <span>
          Search engine URL <span className="muted">(use {"{q}"} for the query)</span>
        </span>
        <input
          className="mono"
          value={p.search_url}
          onChange={(e) => setP({ ...p, search_url: e.target.value })}
        />
      </label>
      <label>
        Default agent
        <select
          value={p.default_agent ?? ""}
          onChange={(e) => setP({ ...p, default_agent: e.target.value || null })}
        >
          <option value="">server default</option>
          {props.agents.map((a) => (
            <option key={a.id} value={a.id}>
              {a.name}
            </option>
          ))}
        </select>
      </label>
      <label>
        Default model
        <select
          value={p.default_model ?? ""}
          onChange={(e) => setP({ ...p, default_model: e.target.value || null })}
        >
          <option value="">server default</option>
          {props.models.map((m) => (
            <option key={m.id} value={m.id}>
              {m.id}
            </option>
          ))}
        </select>
      </label>
      {err && <div className="error-text">{err}</div>}
      {msg && <div className="muted">{msg}</div>}
      <div className="row end">
        <button className="primary">Save</button>
      </div>
    </form>
  );
}

export default function Settings(props: {
  me: Me;
  agents: Agent[];
  models: ModelInfo[];
  onPrefs: (p: Prefs) => void;
}) {
  const [tab, setTab] = useState<Tab>("preferences");
  const tabs: Tab[] = props.me.role === "admin" ? ["preferences", "password", "users"] : ["preferences", "password"];
  return (
    <section className="settings">
      <div className="column wide">
        <h1>{t("settings.title")}</h1>
        <div className="tabs" role="tablist">
          {tabs.map((x) => (
            <button key={x} role="tab" aria-selected={tab === x} className={tab === x ? "on" : ""} onClick={() => setTab(x)}>
              {t(`settings.${x}`)}
            </button>
          ))}
        </div>
        {tab === "preferences" && <Preferences agents={props.agents} models={props.models} onSaved={props.onPrefs} />}
        {tab === "password" && props.me.user_id && (
          <ChangePassword forced={false} onDone={() => setTab("preferences")} />
        )}
        {tab === "users" && <Users me={props.me} />}
      </div>
    </section>
  );
}

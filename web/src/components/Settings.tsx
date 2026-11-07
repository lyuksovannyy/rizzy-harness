// Settings, loaded lazily.
import { useEffect, useState } from "react";
import { api } from "../api";
import { t } from "../lib/i18n";
import type { Agent, Me, ModelInfo, NodeInfo, Prefs } from "../types";
import { ChangePassword } from "./ChangePassword";
import { modelLabel, offeredModels, resolveModel, savablePrefs } from "../lib/models";
import { AgentMark, agentAlert } from "./AgentAvatar";
import { AgentsSettings } from "./AgentsSettings";
import { AppearanceFields } from "./Appearance";
import { ProfileFields, SearchEngineFields } from "./forms";
import { ProvidersSettings } from "./ProviderForm";
import { enableNotify, notifyEnabled, notifySupported } from "../lib/notify";
import { MemoryCfgForm, MemoryPage } from "./Memory";
import { Nodes } from "./Nodes";
import { Select } from "./Select";
import { SavingsSettings } from "./Savings";
import { Users } from "./Users";

export type Tab = "profile" | "preferences" | "agents" | "memory" | "provider" | "savings" | "nodes" | "password" | "users";

function NotifyToggle() {
  const [on, setOn] = useState(notifyEnabled());
  if (!notifySupported()) {
    return (
      <div className="muted small">
        Desktop notifications need a secure page: open this server on localhost, or start it with --tls.
      </div>
    );
  }
  return (
    <label className="check">
      <input type="checkbox" checked={on} onChange={async (e) => setOn(await enableNotify(e.target.checked))} />
      Desktop notifications when a chat needs you (while a tab is open)
    </label>
  );
}

function PrefsEditor(props: {
  initial: Prefs;
  models: ModelInfo[];
  onSaved: (p: Prefs) => void;
  children: (p: Prefs, set: (p: Prefs) => void) => React.ReactNode;
}) {
  const [p, setP] = useState<Prefs>(props.initial);
  const [msg, setMsg] = useState<string | null>(null);
  const [err, setErr] = useState<string | null>(null);
  useEffect(() => setP(props.initial), [props.initial]);
  return (
    <form
      className="card stack"
      onSubmit={async (e) => {
        e.preventDefault();
        setErr(null);
        setMsg(null);
        try {
          const saved = await api.saveSettings(savablePrefs(p, props.models));
          props.onSaved(saved);
          setMsg("Saved.");
        } catch (x) {
          setErr(x instanceof Error ? x.message : String(x));
        }
      }}
    >
      {props.children(p, (n) => {
        setP(n);
        setMsg(null);
      })}
      {err && <div className="error-text">{err}</div>}
      <div className="row end">
        {msg && <span className="muted">{msg}</span>}
        <button className="primary">Save</button>
      </div>
    </form>
  );
}

/** The user's default model: listed models, plus the current one when the
 * provider dropped it, marked, so a notice's "Change your default" shows
 * what to change. */
function DefaultModel(props: { models: ModelInfo[]; value: string | null; onChange: (v: string | null) => void }) {
  const current = resolveModel(props.models, props.value)?.id ?? "";
  return (
    <label>
      Default model
      <select value={current} onChange={(e) => props.onChange(e.target.value || null)}>
        <option value="">server default</option>
        {offeredModels(props.models, current).map((m) => (
          <option key={m.id} value={m.id} disabled={m.unavailable}>
            {modelLabel(m, props.models)}
          </option>
        ))}
      </select>
    </label>
  );
}

export default function Settings(props: {
  me: Me;
  agents: Agent[];
  models: ModelInfo[];
  prefs: Prefs | null;
  onPrefs: (p: Prefs) => void;
  onProviderChanged: () => void;
  /** An agent's look changed: reload the agents. */
  onAgentsChanged?: () => void;
  initialTab?: Tab;
  memoryVersion: number;
  nodes: NodeInfo[];
}) {
  const admin = props.me.role === "admin";
  const tabs: Tab[] = [
    "profile",
    "preferences",
    "agents",
    "memory",
    ...(admin ? (["provider", "savings", "nodes"] as Tab[]) : []),
    ...(props.me.user_id ? (["password"] as Tab[]) : []),
    ...(admin ? (["users"] as Tab[]) : []),
  ];
  const [tab, setTab] = useState<Tab>(props.initialTab ?? "profile");
  useEffect(() => {
    if (props.initialTab) setTab(props.initialTab);
  }, [props.initialTab]);
  const prefs = props.prefs;

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
        {!prefs && <div className="muted">Loading…</div>}
        {prefs && tab === "profile" && (
          <PrefsEditor initial={prefs} models={props.models} onSaved={props.onPrefs}>
            {(p, set) => <ProfileFields prefs={p} onChange={set} username={props.me.username} />}
          </PrefsEditor>
        )}
        {prefs && tab === "preferences" && (
          <PrefsEditor initial={prefs} models={props.models} onSaved={props.onPrefs}>
            {(p, set) => (
              <>
                <AppearanceFields prefs={p} saved={prefs} onChange={set} />
                <h3>Notifications</h3>
                <NotifyToggle />
                <h3>Search engine</h3>
                <SearchEngineFields prefs={p} onChange={set} />
                <div className="form stack">
                  <div className="field">
                    Default agent
                    <Select
                      label="Default agent"
                      value={p.default_agent ?? ""}
                      options={[
                        { value: "", label: "server default" },
                        ...props.agents.map((a) => ({
                          value: a.id,
                          label: a.name,
                          hint: agentAlert(a) ? "model unavailable" : undefined,
                          icon: <AgentMark agent={a} size={18} />,
                        })),
                      ]}
                      onChange={(v) => set({ ...p, default_agent: v || null })}
                    />
                  </div>
                  <DefaultModel models={props.models} value={p.default_model} onChange={(v) => set({ ...p, default_model: v })} />
                </div>
              </>
            )}
          </PrefsEditor>
        )}
        {tab === "memory" && (
          <>
            <MemoryPage version={props.memoryVersion} />
            {admin && (
              <div className="card stack">
                <h3>Recall (server-wide)</h3>
                <MemoryCfgForm />
              </div>
            )}
          </>
        )}
        {tab === "agents" && (
          <AgentsSettings agents={props.agents} admin={admin} onChanged={props.onAgentsChanged ?? props.onProviderChanged} />
        )}
        {tab === "savings" && <SavingsSettings />}
        {tab === "nodes" && <Nodes nodes={props.nodes} />}
        {tab === "provider" && <ProvidersSettings onChanged={props.onProviderChanged} />}
        {tab === "password" && <ChangePassword forced={false} onDone={() => setTab("profile")} />}
        {tab === "users" && <Users me={props.me} />}
      </div>
    </section>
  );
}

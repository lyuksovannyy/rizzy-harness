// Settings, loaded lazily.
import { useEffect, useState } from "react";
import { api } from "../api";
import { t } from "../lib/i18n";
import type { Agent, Me, ModelInfo, Prefs } from "../types";
import { ChangePassword } from "./ChangePassword";
import { ACCENTS, ProfileFields, ProviderForm, SearchEngineFields } from "./forms";
import { Users } from "./Users";

type Tab = "profile" | "preferences" | "provider" | "password" | "users";

function PrefsEditor(props: {
  initial: Prefs;
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
          const saved = await api.saveSettings(p);
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

export default function Settings(props: {
  me: Me;
  agents: Agent[];
  models: ModelInfo[];
  prefs: Prefs | null;
  onPrefs: (p: Prefs) => void;
  onProviderChanged: () => void;
}) {
  const admin = props.me.role === "admin";
  const tabs: Tab[] = [
    "profile",
    "preferences",
    ...(admin ? (["provider"] as Tab[]) : []),
    ...(props.me.user_id ? (["password"] as Tab[]) : []),
    ...(admin ? (["users"] as Tab[]) : []),
  ];
  const [tab, setTab] = useState<Tab>("profile");
  const [providerMsg, setProviderMsg] = useState<string | null>(null);
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
          <PrefsEditor initial={prefs} onSaved={props.onPrefs}>
            {(p, set) => <ProfileFields prefs={p} onChange={set} username={props.me.username} />}
          </PrefsEditor>
        )}
        {prefs && tab === "preferences" && (
          <PrefsEditor initial={prefs} onSaved={props.onPrefs}>
            {(p, set) => (
              <>
                <h3>Search engine</h3>
                <SearchEngineFields prefs={p} onChange={set} />
                <div className="form stack">
                  <label>
                    Default agent
                    <select value={p.default_agent ?? ""} onChange={(e) => set({ ...p, default_agent: e.target.value || null })}>
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
                    <select value={p.default_model ?? ""} onChange={(e) => set({ ...p, default_model: e.target.value || null })}>
                      <option value="">server default</option>
                      {props.models.map((m) => (
                        <option key={m.id} value={m.id}>
                          {m.id}
                        </option>
                      ))}
                    </select>
                  </label>
                  <div>
                    <div className="muted small">Accent</div>
                    <div className="row swatches" role="radiogroup" aria-label="accent">
                      {ACCENTS.map((a) => (
                        <button
                          key={a}
                          type="button"
                          role="radio"
                          aria-checked={(p.accent ?? "blue") === a}
                          aria-label={a}
                          className={`swatch sw-${a} ${(p.accent ?? "blue") === a ? "on" : ""}`}
                          onClick={() => set({ ...p, accent: a === "blue" ? null : a })}
                        />
                      ))}
                    </div>
                  </div>
                </div>
              </>
            )}
          </PrefsEditor>
        )}
        {tab === "provider" && (
          <div className="card stack">
            <ProviderForm
              onSaved={(s) => {
                setProviderMsg(`Saved. New chats use ${s.model}.`);
                props.onProviderChanged();
              }}
            />
            {providerMsg && <div className="muted">{providerMsg}</div>}
          </div>
        )}
        {tab === "password" && <ChangePassword forced={false} onDone={() => setTab("profile")} />}
        {tab === "users" && <Users me={props.me} />}
      </div>
    </section>
  );
}

// Settings, loaded lazily: four sections, one per tier (lib/settings.ts
// decides who sees which tab). One panel is mounted at a time, so leaving a
// tab drops what it did not save (AppearanceFields reverts its preview).
import "../lib/i18nPages";
import { lazy, Suspense, useEffect, useState } from "react";
import { pageApi } from "../apiPages";
import { errText } from "../lib/errorText";
import { t } from "../lib/i18n";
import { routeHash, settingsHash } from "../lib/routes";
import { useRevealTab } from "../lib/revealTab";
import { pickTab, type SectionId, type TabId, visibleSections } from "../lib/settings";
import type { Agent, Me, ModelInfo, NodeInfo, Prefs } from "../types";
import { ChangePassword } from "./ChangePassword";
import { modelLabel, offeredModels, resolveModel } from "../lib/models";
import { savablePrefs } from "../lib/modelPlan";
import { agenticCapable } from "../lib/agentDefaults";
import { AgentMark, agentAlert } from "./AgentAvatar";
import { AppearanceFields } from "./Appearance";
import { SearchEngineFields } from "./forms";
import { ProvidersSettings } from "./ProviderForm";
import { enableNotify, notifyEnabled, notifySupported } from "../lib/notify";
import { MemoryCfgForm, MemoryPage } from "./Memory";
import { Nodes } from "./Nodes";
import { Select } from "./Select";
import { SavingsSettings } from "./Savings";
import { StorageSettings } from "./StorageSettings";
import { UsageSettings } from "./UsageSettings";
import { Users } from "./Users";
import { ServerWorkspaces } from "./workspace/ServerWorkspaces";
import { WorkspaceDelete } from "./workspace/WorkspaceDelete";
import { WorkspaceGeneral } from "./workspace/WorkspaceGeneral";
import { WorkspaceMembers } from "./workspace/WorkspaceMembers";
import { WorkspaceModels } from "./workspace/WorkspaceModels";
import { WorkspaceMemory } from "./workspace/WorkspaceMemory";
import { WorkspaceOverview } from "./workspace/WorkspaceOverview";

// Their own chunks: Profile's pictures, Personalisation, and the workspace's
// agents (the Agents page's list).
const ProfileTab = lazy(() => import("./profile/ProfileTab"));
const PersonalisationTab = lazy(() => import("./profile/PersonalisationTab"));
const WorkspaceAgents = lazy(() => import("./workspace/WorkspaceAgents"));

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
          const saved = await pageApi.saveSettings(savablePrefs(p, props.models));
          props.onSaved(saved);
          setMsg("Saved.");
        } catch (x) {
          setErr(errText(x));
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
        <option value="">{t("settings.workspaceDefault")}</option>
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
  /** In the active workspace's scope, like the rest of the app. */
  agents: Agent[];
  models: ModelInfo[];
  /** The default agent's id in that scope: the workspace's, else the server's. */
  defaultAgent?: string;
  prefs: Prefs | null;
  onPrefs: (p: Prefs) => void;
  onProviderChanged: () => void;
  /** The active workspace changed from a workspace panel. */
  onWorkspaceChanged?: () => void;
  /** The viewer left or deleted the active workspace here. */
  onWorkspaceLeft?: () => void;
  /** The section and tab the route asks for, unchecked; a new object for
   * every hash change, so following the same link twice still opens it. */
  route?: { section?: string; tab?: string; item?: string };
  memoryVersion: number;
  nodes: NodeInfo[];
}) {
  const sections = visibleSections(props.me);
  const [want, setWant] = useState<{ section?: string; tab?: string; item?: string }>(props.route ?? {});
  const [seen, setSeen] = useState(props.route);
  if (seen !== props.route) {
    setSeen(props.route);
    setWant(props.route ?? {});
  }
  // What the viewer may not see falls back to what they may.
  const { section, tab } = pickTab(sections, want);
  const choose = (s: SectionId, x?: TabId) => setWant({ section: s, tab: x });
  // What the Agents tab has open lives in the address too, so a reload comes back to it.
  const item = section === "workspace" && tab === "agents" ? (want.item ?? null) : null;
  const openItem = (id: string | null) => setWant({ section, tab, ...(id ? { item: id } : {}) });
  // The address names the panel shown, so a reload or a copied link comes
  // back to it; replaced, so Back still leaves Settings. Again on every route,
  // since an old path to the tab already shown changes no tab.
  useEffect(() => {
    const h = settingsHash(section, tab, item ?? undefined);
    if (location.hash.startsWith("#/settings") && location.hash !== h) history.replaceState(history.state, "", h);
  }, [section, tab, item, props.route]);
  const tabs = sections.find((s) => s.id === section)!.tabs;
  const tabRow = useRevealTab(`${section}/${tab}`);
  const prefs = props.prefs;
  const ws = props.me.workspace;
  const wsChanged = props.onWorkspaceChanged ?? (() => {});
  const wsProps = ws && {
    me: props.me,
    workspace: ws,
    agents: props.agents,
    models: props.models,
    defaultAgent: props.defaultAgent ?? "general",
    onChanged: wsChanged,
    // Left or deleted: home, where reloading `me` lands in Personal.
    onLeft: () => {
      location.hash = routeHash({ kind: "home" });
      (props.onWorkspaceLeft ?? wsChanged)();
    },
  };

  return (
    <section className="settings">
      <div className="column wide">
        <h1>{t("settings.title")}</h1>
        {sections.length > 1 && (
          <nav className="settings-sections" aria-label="settings sections">
            {sections.map((s) => (
              <button
                key={s.id}
                type="button"
                aria-current={section === s.id ? "page" : undefined}
                className={section === s.id ? "on" : ""}
                onClick={() => choose(s.id)}
              >
                {t(`settings.section.${s.id}`)}
              </button>
            ))}
          </nav>
        )}
        <div className="tabs" role="tablist" ref={tabRow}>
          {tabs.map((x) => (
            <button key={x} role="tab" aria-selected={tab === x} className={tab === x ? "on" : ""} onClick={() => choose(section, x)}>
              {t(`settings.${x}`)}
            </button>
          ))}
        </div>
        {!prefs && (tab === "profile" || tab === "personalisation" || tab === "preferences") && <div className="muted">Loading…</div>}
        <Suspense fallback={<div className="muted">Loading…</div>}>
          {prefs && tab === "profile" && <ProfileTab me={props.me} prefs={prefs} models={props.models} onSaved={props.onPrefs} />}
          {prefs && tab === "personalisation" && <PersonalisationTab prefs={prefs} onSaved={props.onPrefs} />}
          {wsProps && tab === "agents" && <WorkspaceAgents {...wsProps} open={item} onOpen={openItem} />}
        </Suspense>
        {prefs && tab === "preferences" && (
          <PrefsEditor initial={prefs} models={props.models} onSaved={props.onPrefs}>
            {(p, set) => (
              <>
                <AppearanceFields prefs={p} saved={prefs} onChange={set} />
                <h3>Notifications</h3>
                <NotifyToggle />
                <h3>Search engine</h3>
                <SearchEngineFields prefs={p} onChange={set} />
                <h3>New chats</h3>
                <div className="form stack">
                  <div className="field">
                    {t("settings.defaultChat")}
                    <Select
                      label={t("settings.defaultChat")}
                      value={p.default_agent ?? ""}
                      options={[
                        { value: "", label: t("settings.workspaceDefault") },
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
                  <div className="field">
                    {t("settings.defaultAgentic")}
                    <Select
                      label={t("settings.defaultAgentic")}
                      value={p.default_agentic_agent ?? ""}
                      options={[
                        { value: "", label: t("settings.workspaceDefault") },
                        ...agenticCapable(props.agents).map((a) => ({
                          value: a.id,
                          label: a.name,
                          hint: agentAlert(a) ? "model unavailable" : undefined,
                          icon: <AgentMark agent={a} size={18} />,
                        })),
                      ]}
                      onChange={(v) => set({ ...p, default_agentic_agent: v || null })}
                    />
                  </div>
                  <DefaultModel models={props.models} value={p.default_model} onChange={(v) => set({ ...p, default_model: v })} />
                  <span className="field-hint">{t("settings.defaultsHint")}</span>
                </div>
              </>
            )}
          </PrefsEditor>
        )}
        {tab === "memory" && <MemoryPage version={props.memoryVersion} />}
        {tab === "password" && <ChangePassword forced={false} onDone={() => choose("account", "profile")} />}
        {wsProps && tab === "overview" && <WorkspaceOverview {...wsProps} />}
        {wsProps && tab === "facts" && <WorkspaceMemory {...wsProps} version={props.memoryVersion} />}
        {wsProps && tab === "general" && <WorkspaceGeneral {...wsProps} />}
        {wsProps && tab === "models" && <WorkspaceModels {...wsProps} />}
        {wsProps && tab === "members" && <WorkspaceMembers {...wsProps} />}
        {wsProps && tab === "delete" && <WorkspaceDelete {...wsProps} />}
        {tab === "providers" && <ProvidersSettings onChanged={props.onProviderChanged} />}
        {tab === "recall" && (
          <div className="card stack">
            <h3>Recall (server-wide)</h3>
            <MemoryCfgForm />
          </div>
        )}
        {tab === "savings" && <SavingsSettings models={props.models} />}
        {tab === "nodes" && <Nodes nodes={props.nodes} />}
        {tab === "users" && <Users me={props.me} />}
        {tab === "workspaces" && <ServerWorkspaces me={props.me} onChanged={wsChanged} />}
        {tab === "storage" && <StorageSettings me={props.me} />}
        {tab === "usage" && <UsageSettings models={props.models} />}
      </div>
    </section>
  );
}

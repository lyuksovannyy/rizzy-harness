// The admin's AI providers, shared by onboarding and Settings. Each
// provider's model ids are prefixed with its id (`openai::gpt-4o`); custom
// endpoints are named and prefixed by the admin.
import { Info, Pencil, Plus, Trash2, TriangleAlert } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { api } from "../api";
import type { Agent, ProviderModel, ProviderPreset, ProvidersState, ProviderState } from "../types";
import { ModelPicker } from "./ModelPicker";

/** The built-in demo provider's id. */
const DEMO = "demo";
/** Prefixes the server keeps for itself (presets::RESERVED_IDS). */
const RESERVED = [DEMO, "test"];

export const PREFIX_RE = /^[a-z0-9][a-z0-9_-]{0,31}$/;

const GENERIC = new Set(["api", "www", "gateway", "inference"]);
const SECOND_LEVEL = new Set(["co", "com", "net", "org", "ac", "gov"]);

/** A name and a prefix for a custom endpoint, from its URL's host:
 * https://api.omniroute.dev/v1 gives Omniroute and omniroute. */
export function nameFromUrl(url: string): { name: string; prefix: string } {
  let host: string;
  try {
    host = new URL(url.includes("://") ? url : `http://${url}`).hostname;
  } catch {
    return { name: "", prefix: "" };
  }
  host = host.replace(/^\[|\]$/g, "");
  let label = host;
  if (!/^[\d.:]+$/.test(host)) {
    let parts = host.split(".").filter(Boolean);
    if (parts.length > 1) parts = parts.slice(0, -1);
    if (parts.length > 1 && SECOND_LEVEL.has(parts[parts.length - 1]!)) parts = parts.slice(0, -1);
    const named = parts.filter((p) => !GENERIC.has(p));
    label = (named.length ? named : parts).pop() ?? "";
  }
  const prefix = label
    .toLowerCase()
    .replace(/[^a-z0-9_-]+/g, "-")
    .replace(/^[^a-z0-9]+/, "")
    .slice(0, 32);
  return { name: label ? label[0]!.toUpperCase() + label.slice(1) : "", prefix };
}

const secondsLeft = (until: number | null) =>
  until === null ? 0 : Math.max(0, Math.ceil((until - Date.now()) / 1000));

/** Whole seconds left until `until` (ms since the epoch), ticking. Read
 * during render, so the first render with a new `until` already counts. */
export function useCountdown(until: number | null): number {
  const [, tick] = useState(0);
  useEffect(() => {
    if (secondsLeft(until) === 0) return;
    const id = setInterval(() => {
      tick((t) => t + 1);
      if (secondsLeft(until) === 0) clearInterval(id);
    }, 250);
    return () => clearInterval(id);
  }, [until]);
  return secondsLeft(until);
}

/** The host a URL points at, for telling endpoints apart. */
const hostOf = (url: string) => {
  try {
    return new URL(url.includes("://") ? url : `http://${url}`).host;
  } catch {
    return url.trim();
  }
};

/** The fixed preset's own saved entry: a custom endpoint that happens to
 * share its id is someone else. */
const ownEntry = (state: ProvidersState, p: ProviderPreset | undefined) =>
  p && !p.custom ? state.providers.find((x) => x.id === p.id && x.preset === p.id) : undefined;

const WARNING_TITLE = {
  trusted_source: "Connect only endpoints you trust",
  training_data: "Your data may train models",
  data_collection: "Your data may be collected",
} as const;

const bare = (raw: string): ProviderModel => ({
  id: raw,
  name: null,
  context: null,
  max_output: null,
  effort: null,
  vision: null,
  tools: null,
});

/** Agent id to raw model id, for the agents whose default is one of `pid`'s models. */
function assignedTo(state: ProvidersState, pid: string): Record<string, string> {
  const out: Record<string, string> = {};
  for (const [agent, model] of Object.entries(state.agent_models)) {
    if (model.startsWith(`${pid}::`)) out[agent] = model.slice(pid.length + 2);
  }
  return out;
}

/** Add a provider (no `edit`) or change one: pick a preset, test the
 * connection (lists models), choose models and their agents, save. */
export function ProviderForm(props: {
  state: ProvidersState;
  agents: Agent[];
  edit?: ProviderState | null;
  onSaved: (s: ProvidersState) => void;
  onCancel?: () => void;
  saveLabel?: string;
}) {
  const { state, agents, edit } = props;
  const presets = state.presets;
  const [presetId, setPresetId] = useState(
    () => edit?.preset ?? presets.find((p) => !p.custom)?.id ?? presets[0]?.id ?? "",
  );
  const preset: ProviderPreset | undefined = presets.find((p) => p.id === presetId);
  // The provider a save replaces: the one being edited, or a fixed preset's.
  const existing = edit ?? ownEntry(state, preset);
  const [base, setBase] = useState(() => edit?.base_url ?? preset?.base_url ?? "");
  const [name, setName] = useState(edit?.name ?? "");
  const [prefix, setPrefix] = useState(edit?.id ?? "");
  const [touched, setTouched] = useState({ name: !!edit, prefix: !!edit });
  const [key, setKey] = useState("");
  const [listing, setListing] = useState<ProviderModel[] | null>(existing?.models ?? null);
  const [chosen, setChosen] = useState<string[]>(() => existing?.models.map((m) => m.id) ?? []);
  const [assigned, setAssigned] = useState<Record<string, string>>(() =>
    existing ? assignedTo(state, existing.id) : {},
  );
  const [tested, setTested] = useState<{ sig: string; at: number } | null>(null);
  const [msg, setMsg] = useState<string | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const custom = !!preset?.custom;
  const pid = edit?.id ?? (custom ? prefix : (preset?.id ?? ""));
  // A fixed preset's URL is the server's; a client one would be ignored.
  const url = preset?.url_editable ? base.trim() : (preset?.base_url ?? "");
  const sig = JSON.stringify([presetId, url, key]);
  const changed = !existing || key !== "" || url !== existing.base_url || presetId !== existing.preset;
  const wait = tested && tested.sig === sig && preset?.warning ? tested.at + preset.warning.wait_secs * 1000 : null;
  const left = useCountdown(wait);

  const pickPreset = (id: string) => {
    const p = presets.find((x) => x.id === id);
    const ex = ownEntry(state, p);
    setPresetId(id);
    setBase(ex?.base_url ?? p?.base_url ?? "");
    setListing(ex?.models ?? null);
    setChosen(ex?.models.map((m) => m.id) ?? []);
    setAssigned(ex ? assignedTo(state, ex.id) : {});
    setTouched({ name: false, prefix: false });
    setName("");
    setPrefix("");
    // A key typed for one provider never goes to another.
    setKey("");
    setTested(null);
    setMsg(null);
    setErr(null);
  };

  const editBase = (v: string) => {
    if (hostOf(v) !== hostOf(base)) setKey("");
    setBase(v);
    if (!custom || edit) return;
    const auto = nameFromUrl(v);
    if (!touched.name) setName(auto.name);
    if (!touched.prefix) setPrefix(auto.prefix);
  };

  let invalid: string | null = null;
  if (custom && !edit) {
    if (!name.trim()) invalid = "Give the endpoint a name.";
    else if (!PREFIX_RE.test(prefix)) invalid = "The prefix takes a-z, 0-9, - and _, starting with a letter or digit.";
    else if (state.providers.some((p) => p.id === prefix)) invalid = `The prefix ${prefix} is taken by another provider.`;
    else if (RESERVED.includes(prefix) || presets.some((p) => !p.custom && p.id === prefix))
      invalid = `The prefix ${prefix} is reserved for a built-in provider.`;
  } else if (!edit && preset && !custom && state.providers.some((p) => p.id === preset.id && p.preset !== preset.id)) {
    invalid = `The prefix ${preset.id} is used by another provider. Rename or remove it first.`;
  }

  let keyNote = "· stored only on this server (0600)";
  if (existing && existing.key !== "none") {
    if (url !== existing.base_url) keyNote = "· a new endpoint needs its key again";
    else if (existing.key.startsWith("env:")) keyNote = `· read from ${existing.key.slice(4)}; leave empty to keep`;
    else keyNote = "· a key is stored; leave empty to keep";
  }
  const keyKept = keyNote.includes("leave empty");

  const test = async () => {
    if (!preset) return;
    setBusy(true);
    setErr(null);
    setMsg(null);
    try {
      const r = await api.testProvider({
        preset: preset.id,
        base_url: preset.url_editable ? url : undefined,
        api_key: key || undefined,
        provider_id: existing?.id,
      });
      if (!r.ok) throw new Error(r.error || "The test failed.");
      const ids = new Set(r.models.map((m) => m.id));
      setListing(r.models);
      setChosen((c) => c.filter((id) => ids.has(id)));
      setAssigned((a) => Object.fromEntries(Object.entries(a).filter(([, m]) => ids.has(m))));
      setTested({ sig, at: Date.now() });
      setMsg(`Connected. ${r.models.length} models available.`);
    } catch (x) {
      setTested(null);
      setErr(x instanceof Error ? x.message : String(x));
    } finally {
      setBusy(false);
    }
  };

  const save = async () => {
    if (!preset || !pid || secondsLeft(wait) > 0) return;
    setBusy(true);
    setErr(null);
    try {
      const byId = new Map([...(existing?.models ?? []), ...(listing ?? [])].map((m) => [m.id, m]));
      const s = await api.saveProvider(pid, {
        name: custom ? name.trim() : (edit?.name ?? preset.label),
        preset: preset.id,
        base_url: preset.url_editable ? url : undefined,
        api_key: key || undefined,
        models: chosen.map((id) => byId.get(id) ?? bare(id)),
      });
      // This provider's agents as picked here; whoever was on one of its
      // models and is not any more is let go.
      const known = new Set(agents.map((a) => a.id));
      const next: Record<string, string | null> = {};
      for (const a of known) {
        if (assigned[a] && chosen.includes(assigned[a])) next[a] = `${pid}::${assigned[a]}`;
        else if (s.agent_models[a]?.startsWith(`${pid}::`)) next[a] = null;
      }
      // Older configs set only default_model; General keeps that one unless
      // it is the demo's or it is assigned here.
      const general = "general" in next ? next.general : s.agent_models.general;
      const was = s.default_model;
      const current =
        was && !was.startsWith(`${DEMO}::`) && (!was.startsWith(`${pid}::`) || chosen.includes(was.slice(pid.length + 2)))
          ? was
          : null;
      if (known.has("general") && !general) next.general = current ?? `${pid}::${chosen[0]}`;
      const moveDefault = !!next.general && (!current || ("general" in next && !!assigned.general));
      const done = Object.keys(next).length
        ? await api.saveAgentModels({
            agent_models: next,
            ...(moveDefault ? { default_model: next.general } : {}),
          })
        : s;
      setKey("");
      props.onSaved(done);
    } catch (x) {
      setErr(x instanceof Error ? x.message : String(x));
    } finally {
      setBusy(false);
    }
  };

  const assign = (agent: string, model: string | null) =>
    setAssigned((a) => {
      const n = { ...a };
      if (model) n[agent] = model;
      else delete n[agent];
      return n;
    });

  const pickModels = (ids: string[]) => {
    setChosen(ids);
    setAssigned((a) => Object.fromEntries(Object.entries(a).filter(([, m]) => ids.includes(m))));
  };

  const customs = presets.filter((p) => p.custom);
  const known = presets.filter((p) => !p.custom);
  const needsTest = changed && tested?.sig !== sig;
  const canSave = !busy && !!preset && chosen.length > 0 && !invalid && !needsTest && left === 0;

  return (
    <div className="form stack">
      <label>
        Provider
        <select value={presetId} disabled={!!edit} onChange={(e) => pickPreset(e.target.value)}>
          <optgroup label="Your own endpoint">
            {customs.map((p) => (
              <option key={p.id} value={p.id}>
                {p.label}
              </option>
            ))}
          </optgroup>
          <optgroup label="Known providers">
            {known.map((p) => (
              <option key={p.id} value={p.id}>
                {p.label}
              </option>
            ))}
          </optgroup>
        </select>
      </label>
      {preset?.warning && (
        <div className={`callout ${preset.warning.kind === "trusted_source" ? "callout-note" : "callout-warning"}`} role="note">
          <div className="callout-head">
            {preset.warning.kind === "trusted_source" ? <Info size={14} /> : <TriangleAlert size={14} />}
            <span className="callout-title">{WARNING_TITLE[preset.warning.kind]}</span>
          </div>
          <div className="small">{preset.warning.text}</div>
        </div>
      )}
      {preset?.url_editable ? (
        <label>
          API endpoint
          <input
            className="mono"
            value={base}
            onChange={(e) => editBase(e.target.value)}
            placeholder="https://…/v1"
            autoComplete="off"
          />
        </label>
      ) : (
        <label>
          API endpoint
          <input className="mono" value={preset?.base_url ?? ""} readOnly aria-readonly="true" />
        </label>
      )}
      {custom && (
        <div className="row provider-names">
          <label className="grow">
            Name
            <input
              value={name}
              placeholder="My endpoint"
              onChange={(e) => {
                setName(e.target.value);
                setTouched((t) => ({ ...t, name: true }));
              }}
            />
          </label>
          <label className="grow">
            Prefix
            <input
              className="mono"
              value={prefix}
              readOnly={!!edit}
              placeholder="my-endpoint"
              onChange={(e) => {
                setPrefix(e.target.value);
                setTouched((t) => ({ ...t, prefix: true }));
              }}
            />
            <span className="field-hint">
              Model ids read <span className="mono">{prefix || "prefix"}::model</span>
            </span>
          </label>
        </div>
      )}
      <label>
        <span>
          API key {!preset?.needs_key && <span className="muted">(optional) </span>}
          <span className="muted">{keyNote}</span>
        </span>
        <input
          type="password"
          autoComplete="off"
          value={key}
          onChange={(e) => setKey(e.target.value)}
          placeholder={keyKept ? "••••••••" : "sk-…"}
        />
      </label>
      {preset?.kind === "anthropic" && (
        <p className="muted small">
          Anthropic is supported with an API key only. Claude subscription logins cannot be used by third-party tools.
        </p>
      )}
      <div className="row">
        <button type="button" onClick={test} disabled={busy || !preset || !url}>
          Test connection
        </button>
        {msg && <span className="muted">{msg}</span>}
      </div>
      {listing && (
        <>
          <div className="muted small">
            Pick the models to offer. Each agent starts new chats on the model it is the default for; General's is
            the default for everyone.
          </div>
          <ModelPicker
            label="Models"
            models={listing}
            selected={chosen}
            onChange={pickModels}
            agents={agents.length ? { agents, assigned, onAssign: assign } : undefined}
          />
        </>
      )}
      {invalid && (name || prefix || !custom) && <div className="error-text">{invalid}</div>}
      {err && <div className="error-text">{err}</div>}
      <div className="row end">
        {needsTest && listing && <span className="muted small">Test the connection to save.</span>}
        {props.onCancel && (
          <button type="button" onClick={props.onCancel}>
            Cancel
          </button>
        )}
        <button className="primary" type="button" onClick={save} disabled={!canSave}>
          {left > 0 ? `Continue in ${left}s` : (props.saveLabel ?? "Save provider")}
        </button>
      </div>
    </div>
  );
}

/** Loads the providers and agents for a form. */
function useProviders() {
  const [state, setState] = useState<ProvidersState | null>(null);
  const [agents, setAgents] = useState<Agent[]>([]);
  const [err, setErr] = useState<string | null>(null);
  const load = useCallback(() => {
    api
      .providers()
      .then(setState)
      .catch((x) => setErr(x instanceof Error ? x.message : String(x)));
    api
      .agents()
      .then((r) => setAgents(r.agents))
      .catch(() => {});
  }, []);
  useEffect(load, [load]);
  return { state, setState, agents, err };
}

/** Onboarding: connect the first provider. */
export function ProviderSetup(props: { onSaved: (s: ProvidersState) => void; saveLabel?: string }) {
  const { state, agents, err } = useProviders();
  if (!state) return err ? <div className="error-text">{err}</div> : <div className="muted">Loading…</div>;
  return <ProviderForm state={state} agents={agents} onSaved={props.onSaved} saveLabel={props.saveLabel} />;
}

/** Settings: the providers, each editable, and adding another. */
export function ProvidersSettings(props: { onChanged: () => void }) {
  const { state, setState, agents, err } = useProviders();
  const [editing, setEditing] = useState<string | null>(null);
  const [msg, setMsg] = useState<string | null>(null);
  const [delErr, setDelErr] = useState<string | null>(null);
  if (!state) return err ? <div className="error-text">{err}</div> : <div className="muted">Loading…</div>;
  const saved = (s: ProvidersState) => {
    setState(s);
    setEditing(null);
    setMsg(s.default_model ? `Saved. New chats use ${s.default_model}.` : "Saved.");
    props.onChanged();
  };
  const remove = async (p: ProviderState) => {
    if (!confirm(`Remove ${p.name}? Chats can no longer use its models.`)) return;
    setDelErr(null);
    try {
      saved(await api.deleteProvider(p.id));
    } catch (x) {
      setDelErr(x instanceof Error ? x.message : String(x));
    }
  };
  const edit = editing && editing !== "new" ? state.providers.find((p) => p.id === editing) : null;
  return (
    <div className="stack">
      <div className="card stack">
        <h3>Providers</h3>
        {state.providers.length === 0 && <div className="muted">None yet.</div>}
        <ul className="providers">
          {state.providers.map((p) => (
            <li key={p.id}>
              <div className="grow">
                <div>
                  {p.kind === "mock" ? "Built-in demo (no real AI)" : p.name} <span className="mono muted">{p.id}::</span>
                </div>
                <div className="muted small mono ellipsis">
                  {p.base_url || "built in"} · {p.models.length} {p.models.length === 1 ? "model" : "models"}
                </div>
              </div>
              {p.kind !== "mock" && (
                <button type="button" className="icon" aria-label={`Edit ${p.name}`} onClick={() => setEditing(p.id)}>
                  <Pencil size={16} />
                </button>
              )}
              <button type="button" className="icon" aria-label={`Remove ${p.name}`} onClick={() => void remove(p)}>
                <Trash2 size={16} />
              </button>
            </li>
          ))}
        </ul>
        {delErr && <div className="error-text">{delErr}</div>}
        {msg && <div className="muted">{msg}</div>}
        {!editing && (
          <div>
            <button type="button" onClick={() => setEditing("new")}>
              <Plus size={14} /> Add a provider
            </button>
          </div>
        )}
      </div>
      {editing && (
        <div className="card stack">
          <h3>{edit ? `Edit ${edit.name}` : "Add a provider"}</h3>
          <ProviderForm
            key={editing}
            state={state}
            agents={agents}
            edit={edit}
            onSaved={saved}
            onCancel={() => setEditing(null)}
          />
        </div>
      )}
    </div>
  );
}

/** Use the built-in demo provider (no AI) for now. */
export function chooseDemoProvider(): Promise<ProvidersState> {
  return api.demoProvider();
}

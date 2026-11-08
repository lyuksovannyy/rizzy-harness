// The admin's AI providers, shared by onboarding and Settings. Each
// provider's model ids are prefixed with its id (`openai::gpt-4o`); custom
// endpoints are named and prefixed by the admin.
import { ArrowRight, CircleAlert, Info, Loader2, Pencil, Plus, Trash2, TriangleAlert } from "lucide-react";
import { type ReactNode, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api } from "../api";
import { dropModels, embedFirst, type Plan, planChanges, planFrom, planProblems, renameModel, setMain } from "../lib/models";
import type { Agent, ProviderModel, ProviderPreset, ProvidersState, ProviderState } from "../types";
import { type CustomModels, ModelPicker } from "./ModelPicker";

/** The built-in demo provider's id. */
const DEMO = "demo";
/** Prefixes the server keeps for itself (presets::RESERVED_IDS). */
const RESERVED = [DEMO, "test", "models"];

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
  custom: false,
  unavailable: false,
  missing_since: null,
});

/** Saved URLs come back normalised; a trailing slash is not a new endpoint. */
const sameUrl = (a: string, b: string) => a.trim().replace(/\/+$/, "") === b.trim().replace(/\/+$/, "");

/** The plan's id for this form's provider's models: its prefix may still be
 * typed, so `::raw` until it is saved. */
const LOCAL = (raw: string) => `::${raw}`;
const isDemo = (id: string) => id.startsWith(`${DEMO}::`);

/** Add a provider (no `edit`) or change one: pick a preset, test the
 * connection (lists models), choose models and their agents, save. For
 * memory (`purpose` "embeddings") it registers a provider and picks one
 * embeddings model instead: no agents, and its chat models stay as they
 * are. */
export function ProviderForm(props: {
  state: ProvidersState;
  agents: Agent[];
  edit?: ProviderState | null;
  /** `id` is the provider saved; `embeddings` the model picked for memory. */
  onSaved: (s: ProvidersState, saved: { id: string; embeddings: string | null }) => void;
  /** The state changed without a save (a model removed). */
  onState?: (s: ProvidersState) => void;
  /** The provider's listing was refreshed, so its unavailable marks may have moved. */
  onListed?: () => void;
  onCancel?: () => void;
  saveLabel?: string;
  purpose?: "chat" | "embeddings";
  /** Extra buttons at the start of the save row (onboarding's Back). */
  actions?: ReactNode;
}) {
  const { state, agents, edit } = props;
  const embeddings = props.purpose === "embeddings";
  const presets = state.presets;
  const [presetId, setPresetId] = useState(
    () =>
      edit?.preset ??
      (embeddings ? presets.find((p) => p.id === "custom_openai")?.id : undefined) ??
      presets.find((p) => !p.custom && p.group !== "own")?.id ??
      presets[0]?.id ??
      "",
  );
  const preset: ProviderPreset | undefined = presets.find((p) => p.id === presetId);
  // Saved in this form (onboarding comes back to it): edited from then on.
  const [savedAs, setSavedAs] = useState<string | null>(null);
  // The provider a save replaces: the one edited or saved here, or a fixed preset's.
  const existing =
    edit ?? (savedAs ? state.providers.find((p) => p.id === savedAs) : undefined) ?? ownEntry(state, preset);
  const planFor = (local: string | null) => planFrom(state.agent_models, state.default_model, local, isDemo);
  const [base, setBase] = useState(() => edit?.base_url ?? preset?.base_url ?? "");
  const [name, setName] = useState(edit?.name ?? "");
  const [prefix, setPrefix] = useState(edit?.id ?? "");
  const [touched, setTouched] = useState({ name: !!edit, prefix: !!edit });
  const [key, setKey] = useState("");
  // The provider's saved models (and models added by hand), and its latest listing.
  const [own, setOwn] = useState<ProviderModel[]>(() => (embeddings ? [] : (existing?.models ?? [])));
  const [listing, setListing] = useState<ProviderModel[] | null>(null);
  const [fetched, setFetched] = useState<number | null>(null);
  const [chosen, setChosen] = useState<string[]>(() => (embeddings ? [] : (existing?.models.map((m) => m.id) ?? [])));
  const [plan, setPlan] = useState<Plan>(() => planFor(existing?.id ?? null));
  const [tested, setTested] = useState<{ sig: string; at: number } | null>(null);
  const [msg, setMsg] = useState<string | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState<"test" | "refresh" | "save" | null>(null);
  // Which provider's cached listing is wanted; a late answer for another is dropped.
  const want = useRef<string | null>(null);

  const custom = !!preset?.custom;
  const pid = existing?.id ?? (custom ? prefix : (preset?.id ?? ""));
  // A fixed preset's URL is the server's; a client one would be ignored.
  const url = preset?.url_editable ? base.trim() : (preset?.base_url ?? "");
  const sig = JSON.stringify([presetId, url, key]);
  const changed = !existing || key !== "" || !sameUrl(url, existing.base_url) || presetId !== existing.preset;
  const wait = tested && tested.sig === sig && preset?.warning ? tested.at + preset.warning.wait_secs * 1000 : null;
  const left = useCountdown(wait);

  /** The server's cached listing of a saved provider; it lists only when it has none. */
  const loadCache = useCallback(async (id: string) => {
    want.current = id;
    try {
      let l = await api.providerModels(id);
      if (want.current !== id) return;
      if (!l.models.length) l = await api.refreshProviderModels(id);
      if (want.current !== id) return;
      setListing(l.models);
      setFetched(l.fetched_ms);
    } catch {
      // The saved models still show; a refresh or a test lists again.
    }
  }, []);
  const first = useRef(existing?.id ?? null);
  useEffect(() => {
    if (first.current) void loadCache(first.current);
  }, [loadCache]);

  const pickPreset = (id: string) => {
    const p = presets.find((x) => x.id === id);
    const ex = ownEntry(state, p);
    setPresetId(id);
    setSavedAs(null);
    setBase(ex?.base_url ?? p?.base_url ?? "");
    setOwn(embeddings ? [] : (ex?.models ?? []));
    setChosen(embeddings ? [] : (ex?.models.map((m) => m.id) ?? []));
    setPlan(planFor(ex?.id ?? null));
    setListing(null);
    setFetched(null);
    want.current = null;
    if (ex) void loadCache(ex.id);
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
    if (!custom || existing) return;
    const auto = nameFromUrl(v);
    if (!touched.name) setName(auto.name);
    if (!touched.prefix) setPrefix(auto.prefix);
  };

  let invalid: string | null = null;
  if (custom && !existing) {
    if (!name.trim()) invalid = "Give the endpoint a name.";
    else if (!PREFIX_RE.test(prefix)) invalid = "The prefix takes a-z, 0-9, - and _, starting with a letter or digit.";
    else if (state.providers.some((p) => p.id === prefix)) invalid = `The prefix ${prefix} is taken by another provider.`;
    else if (RESERVED.includes(prefix) || presets.some((p) => !p.custom && p.id === prefix))
      invalid = `The prefix ${prefix} is reserved for a built-in provider.`;
  } else if (!existing && preset && !custom && state.providers.some((p) => p.id === preset.id && p.preset !== preset.id)) {
    invalid = `The prefix ${preset.id} is used by another provider. Rename or remove it first.`;
  }

  let keyNote = "· stored only on this server (0600)";
  if (existing && existing.key !== "none") {
    if (!sameUrl(url, existing.base_url)) keyNote = "· a new endpoint needs its key again";
    else if (existing.key.startsWith("env:")) keyNote = `· read from ${existing.key.slice(4)}; leave empty to keep`;
    else keyNote = "· a key is stored; leave empty to keep";
  }
  const keyKept = keyNote.includes("leave empty");

  // Saved models the listing no longer has stay, marked; models added by
  // hand stay as set. Both come first.
  const shown = useMemo(() => {
    const listed = new Set(listing?.map((m) => m.id));
    const byId = new Map<string, ProviderModel>();
    for (const m of own) {
      if (m.custom || !listed.has(m.id)) byId.set(m.id, { ...m, unavailable: !m.custom && (listing ? true : m.unavailable) });
    }
    for (const m of listing ?? []) if (!byId.has(m.id)) byId.set(m.id, m);
    return [...byId.values()];
  }, [own, listing]);
  const shownById = useMemo(() => new Map(shown.map((m) => [m.id, m])), [shown]);

  const test = async () => {
    if (!preset) return;
    setBusy("test");
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
      // Chosen models this endpoint does not have (and the form never saved) go.
      const keep = new Set([...r.models.map((m) => m.id), ...own.map((m) => m.id)]);
      setListing(r.models);
      setFetched(Date.now());
      setChosen((c) => c.filter((id) => keep.has(id)));
      setPlan((p) => dropModels(p, (id) => !id.startsWith("::") || keep.has(id.slice(2))));
      setTested({ sig, at: Date.now() });
      setMsg(`Connected. ${r.models.length} ${r.models.length === 1 ? "model" : "models"}.`);
    } catch (x) {
      setTested(null);
      setErr(x instanceof Error ? x.message : String(x));
    } finally {
      setBusy(null);
    }
  };

  // A saved provider lists again on the server (which keeps the listing);
  // a draft lists through a new test.
  const refresh = async () => {
    if (!existing || changed) return test();
    setBusy("refresh");
    setErr(null);
    try {
      const l = await api.refreshProviderModels(existing.id);
      setListing(l.models);
      setFetched(l.fetched_ms);
      props.onListed?.();
    } catch (x) {
      setErr(x instanceof Error ? x.message : String(x));
    } finally {
      setBusy(null);
    }
  };

  /** The admin's override: the model and its mark leave the provider and every agent. */
  const removeGone = async (raw: string) => {
    if (!existing) return;
    setErr(null);
    try {
      const s = await api.removeProviderModel(existing.id, raw);
      setOwn((o) => o.filter((m) => m.id !== raw));
      setChosen((c) => c.filter((x) => x !== raw));
      // As the server repairs the chain: a first fallback takes the main model's place.
      setPlan((p) => dropModels(p, (id) => id !== LOCAL(raw), true));
      props.onState?.(s);
    } catch (x) {
      setErr(x instanceof Error ? x.message : String(x));
    }
  };

  const save = async () => {
    if (!preset || !pid || secondsLeft(wait) > 0) return;
    setBusy("save");
    setErr(null);
    try {
      const s = await api.saveProvider(pid, {
        name: custom ? name.trim() : (existing?.name ?? preset.label),
        preset: preset.id,
        base_url: preset.url_editable ? url : undefined,
        api_key: key || undefined,
        // Memory's embeddings model is not a chat model.
        models: embeddings ? (existing?.models ?? []) : chosen.map((id) => shownById.get(id) ?? bare(id)),
      });
      setKey("");
      if (!edit) setSavedAs(pid);
      let done = s;
      if (!embeddings && agents.length) {
        const next = planChanges(s.agent_models, plan, pid, agents.map((a) => a.id));
        // General's main model is the default for everyone.
        const general = "general" in next ? (next.general?.model ?? null) : undefined;
        const moveDefault = general !== undefined && general !== null && general !== s.default_model;
        if (Object.keys(next).length) {
          try {
            done = await api.saveAgentModels({ agent_models: next, ...(moveDefault ? { default_model: general } : {}) });
          } catch (x) {
            // The provider is saved: the parent learns of it, the plan's error shows.
            props.onState?.(s);
            throw x;
          }
        }
      }
      props.onSaved(done, { id: pid, embeddings: embeddings ? (chosen[0] ?? null) : null });
    } catch (x) {
      setErr(x instanceof Error ? x.message : String(x));
    } finally {
      setBusy(null);
    }
  };

  // Models set by hand: added (and chosen), edited, removed.
  const reachable = tested?.sig === sig || (!!existing && !changed);
  const customs: CustomModels = {
    onSave: (m, was) => {
      setOwn((o) => [...o.filter((x) => x.id !== m.id && x.id !== was), m]);
      if (was && was !== m.id) {
        setChosen((c) => c.map((x) => (x === was ? m.id : x)));
        setPlan((p) => renameModel(p, LOCAL(was), LOCAL(m.id)));
      } else if (!was) {
        if (embeddings) setChosen([m.id]);
        else {
          setChosen((c) => (c.includes(m.id) ? c : [...c, m.id]));
          if (agents.some((a) => a.id === "general")) setPlan((p) => (p.general?.model ? p : setMain(p, "general", LOCAL(m.id))));
        }
      }
    },
    onRemove: (id) => {
      setOwn((o) => o.filter((x) => x.id !== id));
      setChosen((c) => c.filter((x) => x !== id));
      setPlan((p) => dropModels(p, (x) => x !== LOCAL(id)));
    },
    test:
      reachable && preset
        ? () => ({
            provider_id: existing?.id,
            preset: preset.id,
            base_url: preset.url_editable ? url : undefined,
            api_key: key || undefined,
            kind: preset.kind,
          })
        : null,
  };

  // Other providers' models in a plan read with their prefix; this one's without.
  const label = (id: string) => (id.startsWith("::") ? id.slice(2) : id);
  const gone = (id: string) => {
    if (id.startsWith("::")) return !!shownById.get(id.slice(2))?.unavailable;
    const [p, ...raw] = id.split("::");
    return !!state.providers.find((x) => x.id === p)?.models.find((m) => m.id === raw.join("::"))?.unavailable;
  };

  const groups = [
    { label: "Your own endpoint", list: presets.filter((p) => (p.group ?? (p.custom ? "own" : "known")) === "own") },
    { label: "Known providers", list: presets.filter((p) => (p.group ?? (p.custom ? "own" : "known")) === "known") },
  ];
  const needsTest = changed && tested?.sig !== sig;
  const problems = !embeddings && agents.length ? planProblems(plan, chosen.map(LOCAL), agents, label) : [];
  // A saved provider may end with no chat models (one kept for memory only).
  const enough = embeddings ? chosen.length === 1 : chosen.length > 0 || !!existing;
  const canSave = !busy && !!preset && enough && problems.length === 0 && !invalid && !needsTest && left === 0;
  const testing = busy === "test";
  // Off but still focusable, so focus stays on it while it lists.
  const testOff = !!busy || !preset || !url;

  return (
    <div className="form stack">
      <label>
        Provider
        <select value={presetId} disabled={!!edit} onChange={(e) => pickPreset(e.target.value)}>
          {groups.map(
            (g) =>
              g.list.length > 0 && (
                <optgroup key={g.label} label={g.label}>
                  {g.list.map((p) => (
                    <option key={p.id} value={p.id}>
                      {p.label}
                    </option>
                  ))}
                </optgroup>
              ),
          )}
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
          <input className="mono" value={base} onChange={(e) => editBase(e.target.value)} placeholder="https://…/v1" autoComplete="off" />
        </label>
      ) : (
        <label>
          API endpoint
          {/* Fixed by the preset: read, never focused or edited. */}
          <input className="mono fixed" value={preset?.base_url ?? ""} disabled tabIndex={-1} aria-readonly="true" />
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
              readOnly={!!existing}
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
      <div className="field-action">
        <label className="grow">
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
        <button
          type="button"
          className="icon test-connection"
          aria-label="Test connection"
          title="Test connection"
          aria-disabled={testOff || undefined}
          aria-busy={testing || undefined}
          onClick={() => {
            if (!testOff) void test();
          }}
        >
          {testing ? <Loader2 size={16} className="spin" /> : <ArrowRight size={16} />}
        </button>
      </div>
      {(msg || testing) && (
        <div className="muted small test-status" aria-live="polite">
          {testing ? "Listing models…" : msg}
        </div>
      )}
      {preset?.kind === "anthropic" && (
        <p className="muted small">
          Anthropic is supported with an API key only. Claude subscription logins cannot be used by third-party tools.
        </p>
      )}
      {(listing || own.length > 0) && (
        <>
          <div className="muted small">
            {embeddings
              ? "Pick the embeddings model. Listings often leave them out: add one by hand with the id the provider gives it."
              : "Pick the models to offer and give each to an agent, as its main model or a fallback for when the main one fails. General's main model is the default for everyone."}
          </div>
          <ModelPicker
            label={embeddings ? "Embeddings model" : "Models"}
            mode={embeddings ? "single" : "multi"}
            embeddings={embeddings}
            models={shown}
            sort={embeddings ? embedFirst : undefined}
            selected={chosen}
            onChange={setChosen}
            agents={!embeddings && agents.length ? { agents, plan, onPlan: setPlan, ref: LOCAL, label, unavailable: gone } : undefined}
            custom={customs}
            onRefresh={() => void refresh()}
            refreshing={busy === "refresh" || testing}
            fetchedMs={fetched}
            onRemoveUnavailable={existing && !embeddings ? (raw) => void removeGone(raw) : undefined}
          />
        </>
      )}
      {invalid && (name || prefix || !custom) && <div className="error-text">{invalid}</div>}
      {err && <div className="error-text">{err}</div>}
      <div className={props.actions ? "row" : "row end"}>
        {props.actions}
        {props.actions && <span className="spacer" />}
        {needsTest && (listing || own.length > 0) && <span className="muted small">Test the connection to save.</span>}
        {props.onCancel && (
          <button type="button" onClick={props.onCancel}>
            Cancel
          </button>
        )}
        <button className="primary" type="button" onClick={() => void save()} disabled={!canSave}>
          {left > 0 ? `Continue in ${left}s` : (props.saveLabel ?? "Save provider")}
        </button>
      </div>
    </div>
  );
}

/** Loads the providers and agents for a form (admins only: `enabled`). The
 * agents are the server's: the chains a provider's models are placed in. */
export function useProviders(enabled = true) {
  const [state, setState] = useState<ProvidersState | null>(null);
  const [agents, setAgents] = useState<Agent[]>([]);
  const [err, setErr] = useState<string | null>(null);
  const load = useCallback(() => {
    if (!enabled) return;
    api
      .providers()
      .then(setState)
      .catch((x) => setErr(x instanceof Error ? x.message : String(x)));
    api
      .agents("server")
      .then((r) => setAgents(r.agents))
      .catch(() => {});
  }, [enabled]);
  useEffect(load, [load]);
  return { state, setState, agents, err };
}

/** "gpt-4 is no longer listed", or how many are not. */
function unavailableNote(p: ProviderState): string {
  const gone = p.models.filter((m) => m.unavailable && !m.custom);
  return gone.length === 1
    ? `${gone[0]!.id} is no longer listed. Edit to remove it or keep it.`
    : `${gone.length} models are no longer listed. Edit to remove them or keep them.`;
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
  const changed = (s: ProvidersState) => {
    setState(s);
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
                {p.models.some((m) => m.unavailable && !m.custom) && (
                  <div className="small provider-gone">
                    <CircleAlert size={12} aria-hidden /> {unavailableNote(p)}
                  </div>
                )}
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
            onState={changed}
            onListed={props.onChanged}
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

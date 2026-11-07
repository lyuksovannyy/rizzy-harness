// Memory: what the assistant remembers about you. Proposals from chats wait
// here for a confirm; only confirmed facts are used.
import { type ReactNode, useCallback, useEffect, useState } from "react";
import { api } from "../api";
import type { ApiStyle, FactKind, MemoryCfg, MemoryFact, MemorySource, Preset, ProviderModel, ProviderState } from "../types";
import { ModelPicker } from "./ModelPicker";
import { Seg } from "./Seg";

const KIND_LABEL: Record<FactKind, string> = { fact: "Fact", preference: "Preference", location: "Location" };

function FactRow(props: { f: MemoryFact; onChange: () => void }) {
  const { f } = props;
  const [edit, setEdit] = useState<string | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const run = async (p: Promise<unknown>) => {
    setErr(null);
    try {
      await p;
      setEdit(null);
      props.onChange();
    } catch (x) {
      setErr(x instanceof Error ? x.message : String(x));
    }
  };
  return (
    <li className={`fact ${f.status}`}>
      {edit === null ? (
        <div className="fact-text">{f.text}</div>
      ) : (
        <input className="grow" value={edit} aria-label="memory text" onChange={(e) => setEdit(e.target.value)} />
      )}
      <div className="row fact-meta">
        <span className="muted small">{KIND_LABEL[f.kind]}</span>
        {f.source_session && (
          <a className="small" href={`#/s/${f.source_session}`}>
            from a chat
          </a>
        )}
        <span className="spacer" />
        {f.status === "proposed" && edit === null && (
          <>
            <button type="button" className="primary" onClick={() => void run(api.updateMemory(f.id, { status: "active" }))}>
              Remember
            </button>
            <button type="button" onClick={() => void run(api.deleteMemory(f.id))}>
              Discard
            </button>
          </>
        )}
        {f.status === "active" && edit === null && (
          <>
            <button type="button" className="link" onClick={() => setEdit(f.text)}>
              Edit
            </button>
            <button type="button" className="link" onClick={() => void run(api.deleteMemory(f.id))}>
              Forget
            </button>
          </>
        )}
        {edit !== null && (
          <>
            <button type="button" className="primary" onClick={() => void run(api.updateMemory(f.id, { text: edit }))}>
              Save
            </button>
            <button type="button" onClick={() => setEdit(null)}>
              Cancel
            </button>
          </>
        )}
      </div>
      {err && <div className="error-text">{err}</div>}
    </li>
  );
}

export function MemoryPage(props: { version: number }) {
  const [facts, setFacts] = useState<MemoryFact[] | null>(null);
  const [backend, setBackend] = useState("");
  const [text, setText] = useState("");
  const [err, setErr] = useState<string | null>(null);
  const load = useCallback(() => {
    api
      .memories()
      .then((r) => {
        setFacts(r.memories);
        setBackend(r.backend);
      })
      .catch((x) => setErr(x instanceof Error ? x.message : String(x)));
  }, []);
  useEffect(load, [load, props.version]);
  if (err && !facts) return <div className="card muted">{err}</div>;
  if (!facts) return <div className="muted">Loading…</div>;
  const proposed = facts.filter((f) => f.status === "proposed");
  const active = facts.filter((f) => f.status === "active");
  return (
    <div className="stack">
      <p className="muted">
        Short facts the assistant may use in future chats. It suggests them during chats; nothing is used until
        you choose Remember. Incognito chats never read or write memory.
      </p>
      {proposed.length > 0 && (
        <div className="card stack">
          <h3>To review ({proposed.length})</h3>
          <ul className="facts">
            {proposed.map((f) => (
              <FactRow key={f.id} f={f} onChange={load} />
            ))}
          </ul>
        </div>
      )}
      <div className="card stack">
        <h3>Remembered</h3>
        <form
          className="row"
          onSubmit={async (e) => {
            e.preventDefault();
            setErr(null);
            try {
              await api.addMemory(text);
              setText("");
              load();
            } catch (x) {
              setErr(x instanceof Error ? x.message : String(x));
            }
          }}
        >
          <input className="grow" placeholder="Add something to remember" value={text} onChange={(e) => setText(e.target.value)} />
          <button className="primary" disabled={!text.trim()}>
            Add
          </button>
        </form>
        {err && <div className="error-text">{err}</div>}
        {active.length === 0 ? (
          <div className="muted">Nothing yet.</div>
        ) : (
          <ul className="facts">
            {active.map((f) => (
              <FactRow key={f.id} f={f} onChange={load} />
            ))}
          </ul>
        )}
        <div className="muted small">Stored in: {backend === "builtin" ? "this server" : backend}</div>
      </div>
    </div>
  );
}

/** "embedding" in the id or name first, then "embed", then the rest. */
export function embedRank(m: ProviderModel): number {
  const s = `${m.id} ${m.name ?? ""}`.toLowerCase();
  return s.includes("embedding") ? 0 : s.includes("embed") ? 1 : 2;
}

export const embedFirst = (a: ProviderModel, b: ProviderModel) => embedRank(a) - embedRank(b) || a.id.localeCompare(b.id);

const OLLAMA_URL = "http://localhost:11434/v1";
type PresetPick = Preset | "custom";
const PICK_LABEL: Record<PresetPick, string> = { fast: "Fast", balanced: "Balanced", best: "Best", custom: "Custom" };
const errText = (x: unknown) => (x instanceof Error ? x.message : String(x));

interface Listing {
  /** Which endpoint it lists: `provider:<id>`, `ollama:<url>`, `custom:<url>`. */
  tag: string;
  models: ProviderModel[] | null;
  error: string | null;
  busy: boolean;
}

/** One model listing at a time, through the provider test endpoint. */
function useListing() {
  const [listing, setListing] = useState<Listing | null>(null);
  const list = useCallback(async (tag: string, req: Parameters<typeof api.testProvider>[0]) => {
    setListing({ tag, models: null, error: null, busy: true });
    const done = (l: Omit<Listing, "tag" | "busy">) => setListing((s) => (s?.tag === tag ? { tag, busy: false, ...l } : s));
    try {
      const r = await api.testProvider(req);
      done(r.ok ? { models: r.models, error: null } : { models: null, error: r.error });
    } catch (x) {
      done({ models: null, error: errText(x) });
    }
  }, []);
  return [listing, list] as const;
}

/** Admin: recall by keywords, or by meaning through an embeddings model
 * from a configured provider, Ollama, or another endpoint. */
export function MemoryCfgForm(props: {
  onSaved?: () => void;
  saveLabel?: string;
  /** Extra buttons at the start of the save row (onboarding's Back). */
  actions?: ReactNode;
}) {
  const [cfg, setCfg] = useState<MemoryCfg | null>(null);
  const [providers, setProviders] = useState<ProviderState[]>([]);
  const [recall, setRecall] = useState<MemoryCfg["recall"]>("keywords");
  const [source, setSource] = useState<MemorySource>("ollama");
  const [providerId, setProviderId] = useState("");
  const [ollamaUrl, setOllamaUrl] = useState(OLLAMA_URL);
  const [customUrl, setCustomUrl] = useState("");
  const [style, setStyle] = useState<ApiStyle>("openai");
  const [key, setKey] = useState("");
  const [pick, setPick] = useState<PresetPick>("balanced");
  // Each source keeps its own model id while the admin switches around.
  const [models, setModels] = useState<Record<MemorySource, string>>({ provider: "", ollama: "", custom: "" });
  const [listing, list] = useListing();
  const [busy, setBusy] = useState(false);
  const [msg, setMsg] = useState<string | null>(null);
  const [err, setErr] = useState<string | null>(null);

  useEffect(() => {
    Promise.all([
      api.memoryCfg(),
      api
        .providers()
        .then((s) => s.providers.filter((p) => p.kind !== "mock"))
        .catch(() => [] as ProviderState[]),
    ])
      .then(([c, ps]) => {
        setCfg(c);
        setProviders(ps);
        setRecall(c.recall);
        const usable = c.source !== "provider" || ps.some((p) => p.id === c.provider_id);
        setSource(c.recall === "meaning" && usable ? c.source : ps.length ? "provider" : "ollama");
        setProviderId(ps.some((p) => p.id === c.provider_id) ? c.provider_id! : (ps[0]?.id ?? ""));
        if (c.source === "ollama" && c.embeddings_url) setOllamaUrl(c.embeddings_url);
        if (c.source === "custom") setCustomUrl(c.embeddings_url);
        setStyle(c.api_style);
        setPick(c.source === "ollama" && c.embeddings_model ? "custom" : c.preset);
        setModels((m) => ({ ...m, [c.source]: c.embeddings_model }));
      })
      .catch((x) => setErr(errText(x)));
  }, []);

  const provider = providers.find((p) => p.id === providerId);
  const meaning = recall === "meaning";
  // The provider's listing and Ollama's come by themselves; a custom
  // endpoint is listed on request (it may need a key first).
  useEffect(() => {
    if (!meaning || source !== "provider" || !provider) return;
    void list(`provider:${provider.id}`, { preset: provider.preset, base_url: provider.base_url, provider_id: provider.id });
  }, [meaning, source, provider, list]);
  useEffect(() => {
    if (!meaning || source !== "ollama" || pick !== "custom" || !ollamaUrl.trim()) return;
    const t = setTimeout(() => void list(`ollama:${ollamaUrl.trim()}`, { preset: "ollama", base_url: ollamaUrl.trim() }), 400);
    return () => clearTimeout(t);
  }, [meaning, source, pick, ollamaUrl, list]);

  // Nothing to set here; onboarding still needs a way on.
  const onward = props.onSaved && (
    <div className="row end">
      <button className="primary" type="button" onClick={props.onSaved}>
        Continue
      </button>
    </div>
  );
  if (!cfg) {
    if (!err) return <div className="muted">Loading…</div>;
    return (
      <div className="stack">
        <div className="error-text">{err}</div>
        {onward}
      </div>
    );
  }
  if (cfg.backend !== "builtin") {
    return (
      <div className="stack">
        <div className="muted">Memory uses the {cfg.backend} backend (set in harness.toml).</div>
        {onward}
      </div>
    );
  }

  const model = models[source];
  const setModel = (v: string) => setModels((m) => ({ ...m, [source]: v }));
  const preset = cfg.presets.find((p) => p.id === pick);
  const tag =
    source === "provider" ? `provider:${providerId}` : source === "ollama" ? `ollama:${ollamaUrl.trim()}` : `custom:${style}:${customUrl.trim()}`;
  const shown = listing?.tag === tag ? listing : null;
  const keyNote =
    cfg.source === "custom" && cfg.key === "stored"
      ? customUrl.trim() === cfg.embeddings_url
        ? "· a key is stored; leave empty to keep"
        : "· a new endpoint needs its key again"
      : "· stored only on this server (0600)";
  const needsModel = source !== "ollama" || pick === "custom";
  const ready = !meaning || ((source !== "provider" || !!provider) && (source !== "custom" || !!customUrl.trim()) && (!needsModel || !!model.trim()));

  const picker = (
    <>
      {shown?.busy && <div className="muted small">Listing models…</div>}
      {shown?.error && <div className="error-text small">{shown.error}</div>}
      {!!shown?.models?.length && (
        <ModelPicker
          label="Embeddings model"
          single
          models={shown.models}
          sort={embedFirst}
          selected={model ? [model] : []}
          onChange={(ids) => setModel(ids[0] ?? "")}
        />
      )}
      {/* A listing may leave out embeddings models (many /models list chat
          models only), so an id can always be typed. */}
      <label>
        {shown?.models?.length ? "Or type a model id" : "Embeddings model"}
        <input
          className="mono"
          value={model}
          placeholder={source === "ollama" ? "e.g. nomic-embed-text" : "e.g. text-embedding-3-small"}
          onChange={(e) => setModel(e.target.value)}
        />
      </label>
    </>
  );

  return (
    <form
      className="form stack"
      onSubmit={async (e) => {
        e.preventDefault();
        setBusy(true);
        setErr(null);
        setMsg(null);
        try {
          const keep = pick === "custom" ? cfg.preset : pick;
          const body: Parameters<typeof api.saveMemoryCfg>[0] = { recall, source, preset: keep };
          if (meaning && source === "provider") Object.assign(body, { provider_id: providerId, embeddings_model: model.trim() });
          if (meaning && source === "ollama")
            Object.assign(body, { embeddings_url: ollamaUrl.trim(), embeddings_model: pick === "custom" ? model.trim() : "" });
          if (meaning && source === "custom")
            Object.assign(body, {
              embeddings_url: customUrl.trim(),
              api_style: style,
              embeddings_model: model.trim(),
              ...(key ? { embeddings_key: key } : {}),
            });
          const c = await api.saveMemoryCfg(body);
          setCfg(c);
          setKey("");
          setMsg(c.active_model ? `Using ${c.active_model}.` : "Keyword recall only.");
          props.onSaved?.();
        } catch (x) {
          setErr(errText(x));
        } finally {
          setBusy(false);
        }
      }}
    >
      <div className="field">
        <span>Recall</span>
        <Seg
          label="recall"
          value={recall}
          options={[
            { value: "keywords", label: "Keywords only" },
            { value: "meaning", label: "Meaning" },
          ]}
          onChange={setRecall}
        />
        <span className="field-hint">
          {meaning
            ? "An embeddings model matches memories by what they mean, not only their words."
            : "Matches memories by their words. Nothing else to set up."}
        </span>
      </div>
      {meaning && (
        <>
          <div className="field">
            <span>Embeddings from</span>
            <Seg
              label="embeddings source"
              value={source}
              options={[
                ...(providers.length ? [{ value: "provider" as const, label: provider?.name ?? providers[0]!.name }] : []),
                { value: "ollama", label: "Ollama" },
                { value: "custom", label: "Custom" },
              ]}
              onChange={setSource}
            />
          </div>
          {source === "provider" && (
            <>
              {providers.length > 1 && (
                <label>
                  Provider
                  <select
                    value={providerId}
                    onChange={(e) => {
                      // Another provider's model ids are not this one's.
                      setProviderId(e.target.value);
                      setModels((m) => ({ ...m, provider: "" }));
                    }}
                  >
                    {providers.map((p) => (
                      <option key={p.id} value={p.id}>
                        {p.name}
                      </option>
                    ))}
                  </select>
                </label>
              )}
              <span className="field-hint">Uses {provider?.name}'s endpoint and key. Many providers offer embeddings models.</span>
              {picker}
            </>
          )}
          {source === "ollama" && (
            <>
              <label>
                Ollama endpoint
                <input className="mono" value={ollamaUrl} onChange={(e) => setOllamaUrl(e.target.value)} />
              </label>
              <div className="field">
                <span>Model</span>
                <Seg
                  label="embeddings model preset"
                  value={pick}
                  options={[...cfg.presets.map((p) => ({ value: p.id as PresetPick, label: PICK_LABEL[p.id] })), { value: "custom", label: "Custom" }]}
                  onChange={setPick}
                />
                {preset && (
                  <span className="field-hint">
                    <span className="mono">{preset.model}</span> · {preset.size} download. Pull it first: <span className="mono">ollama pull {preset.model}</span>
                  </span>
                )}
              </div>
              {pick === "custom" && picker}
            </>
          )}
          {source === "custom" && (
            <>
              <label>
                Embeddings endpoint (OpenAI &amp; Anthropic compatible)
                <input className="mono" value={customUrl} placeholder="https://…/v1" onChange={(e) => setCustomUrl(e.target.value)} />
              </label>
              <div className="field">
                <span>API style</span>
                <Seg
                  label="API style"
                  value={style}
                  options={[
                    { value: "openai", label: "OpenAI" },
                    { value: "anthropic", label: "Anthropic" },
                  ]}
                  onChange={setStyle}
                />
              </div>
              <label>
                <span>
                  API key <span className="muted">(optional) {keyNote}</span>
                </span>
                <input type="password" autoComplete="off" value={key} onChange={(e) => setKey(e.target.value)} />
              </label>
              <div className="row">
                <button
                  type="button"
                  disabled={!customUrl.trim() || !!shown?.busy}
                  onClick={() =>
                    void list(tag, {
                      preset: style === "anthropic" ? "custom_anthropic" : "custom_openai",
                      base_url: customUrl.trim(),
                      api_key: key || undefined,
                      memory: true,
                    })
                  }
                >
                  List models
                </button>
              </div>
              {picker}
            </>
          )}
        </>
      )}
      {err && <div className="error-text">{err}</div>}
      <div className={props.actions ? "row" : "row end"}>
        {props.actions}
        {props.actions && <span className="spacer" />}
        {msg && <span className="muted">{msg}</span>}
        <button className="primary" disabled={busy || !ready}>
          {busy ? "Testing…" : (props.saveLabel ?? "Save")}
        </button>
      </div>
    </form>
  );
}

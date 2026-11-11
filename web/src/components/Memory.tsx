// Memory: what the assistant remembers about you. Proposals from chats wait
// here for a confirm; only confirmed facts are used.
import { type ReactNode, useCallback, useEffect, useRef, useState } from "react";
import { api } from "../api";
import { pageApi } from "../apiPages";
import { embedFirst } from "../lib/modelPlan";
import type { FactKind, MemoryCfg, MemoryFact, Preset, ProviderModel, ProvidersState } from "../types";
import { ModelPicker } from "./ModelPicker";
import { ProviderForm } from "./ProviderForm";
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
            <button type="button" className="primary" onClick={() => void run(pageApi.updateMemory(f.id, { status: "active" }))}>
              Remember
            </button>
            <button type="button" onClick={() => void run(pageApi.deleteMemory(f.id))}>
              Discard
            </button>
          </>
        )}
        {f.status === "active" && edit === null && (
          <>
            <button type="button" className="link" onClick={() => setEdit(f.text)}>
              Edit
            </button>
            <button type="button" className="link" onClick={() => void run(pageApi.deleteMemory(f.id))}>
              Forget
            </button>
          </>
        )}
        {edit !== null && (
          <>
            <button type="button" className="primary" onClick={() => void run(pageApi.updateMemory(f.id, { text: edit }))}>
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
              await pageApi.addMemory(text);
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
        <div className="muted small">Stored in: {backend === "builtin" ? "this server" : backend === "qdrant" ? "this server (vectors in Qdrant)" : backend}</div>
      </div>
    </div>
  );
}

const OLLAMA_URL = "http://localhost:11434/v1";
type PresetPick = Preset | "custom";
const PICK_LABEL: Record<PresetPick, string> = { fast: "Fast", balanced: "Balanced", best: "Best", custom: "Custom" };
const errText = (x: unknown) => (x instanceof Error ? x.message : String(x));

/** Where embeddings come from in the form: a configured provider, Ollama,
 * or a provider registered here (saved as a provider source). */
type From = `provider:${string}` | "ollama" | "custom";

interface Listing {
  models: ProviderModel[] | null;
  fetched: number | null;
  error: string | null;
  busy: boolean;
}

/** Model listings by source: a provider's is the server's cache, Ollama's
 * is asked for. Each is fetched once; the picker's refresh asks again. */
function useListings() {
  const [all, setAll] = useState<Record<string, Listing>>({});
  const run = useCallback(async (tag: string, get: () => Promise<{ models: ProviderModel[]; fetched: number | null }>) => {
    setAll((a) => ({ ...a, [tag]: { models: a[tag]?.models ?? null, fetched: a[tag]?.fetched ?? null, error: null, busy: true } }));
    try {
      const r = await get();
      setAll((a) => ({ ...a, [tag]: { ...r, error: null, busy: false } }));
    } catch (x) {
      setAll((a) => ({ ...a, [tag]: { models: a[tag]?.models ?? null, fetched: a[tag]?.fetched ?? null, error: errText(x), busy: false } }));
    }
  }, []);
  return [all, run] as const;
}

const providerList = (id: string, refresh: boolean) => async () => {
  let l = refresh ? await pageApi.refreshProviderModels(id) : await pageApi.providerModels(id);
  // Nothing cached: list once.
  if (!refresh && !l.models.length) l = await pageApi.refreshProviderModels(id);
  return { models: l.models, fetched: l.fetched_ms };
};
const ollamaList = (url: string) => async () => {
  const r = await pageApi.testProvider({ preset: "ollama", base_url: url });
  if (!r.ok) throw new Error(r.error);
  return { models: r.models, fetched: Date.now() };
};

/** A model id the listing does not have, as an entry the picker can show. */
const byHand = (id: string): ProviderModel => ({
  id,
  name: null,
  context: null,
  max_output: null,
  effort: null,
  vision: null,
  tools: null,
  custom: true,
  unavailable: false,
  missing_since: null,
});

/** Admin: recall by keywords, or by meaning through an embeddings model
 * from a configured provider, Ollama, or a provider registered here. */
export function MemoryCfgForm(props: {
  onSaved?: () => void;
  saveLabel?: string;
  /** Extra buttons at the start of the save row (onboarding's Back). */
  actions?: ReactNode;
  /** The providers, when the page keeps them (onboarding); else loaded here. */
  providers?: ProvidersState | null;
  onProviders?: (s: ProvidersState) => void;
}) {
  const [cfg, setCfg] = useState<MemoryCfg | null>(null);
  const [loaded, setLoaded] = useState<ProvidersState | null>(null);
  const pstate = props.providers ?? loaded;
  const providers = pstate?.providers.filter((p) => p.kind !== "mock") ?? [];
  const [recall, setRecall] = useState<MemoryCfg["recall"]>("keywords");
  const [from, setFrom] = useState<From>("ollama");
  const [ollamaUrl, setOllamaUrl] = useState(OLLAMA_URL);
  const [pick, setPick] = useState<PresetPick>("balanced");
  // Each source keeps its own model, and models added by hand, while the
  // admin switches around (keys: `provider:<id>`, `ollama`).
  const [models, setModels] = useState<Record<string, string>>({});
  const [extra, setExtra] = useState<Record<string, ProviderModel[]>>({});
  // The registration form stays as filled once opened.
  const [registering, setRegistering] = useState(false);
  const [listings, list] = useListings();
  const [busy, setBusy] = useState(false);
  const [msg, setMsg] = useState<string | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const given = useRef(props.providers);

  useEffect(() => {
    Promise.all([pageApi.memoryCfg(), given.current ? Promise.resolve(given.current) : pageApi.providers().catch(() => null)])
      .then(([c, ps]) => {
        if (!given.current) setLoaded(ps);
        const list = ps?.providers.filter((p) => p.kind !== "mock") ?? [];
        setCfg(c);
        setRecall(c.recall);
        const meaning = c.recall === "meaning";
        if (meaning && c.source === "provider" && list.some((p) => p.id === c.provider_id)) setFrom(`provider:${c.provider_id}`);
        else if (meaning && c.source === "ollama") setFrom("ollama");
        else setFrom(list[0] ? `provider:${list[0].id}` : "ollama");
        if (c.source === "ollama" && c.embeddings_url) setOllamaUrl(c.embeddings_url);
        setPick(c.source === "ollama" && c.embeddings_model ? "custom" : c.preset);
        if (c.source === "provider" && c.provider_id) setModels({ [`provider:${c.provider_id}`]: c.embeddings_model });
        if (c.source === "ollama") setModels({ ollama: c.embeddings_model });
      })
      .catch((x) => setErr(errText(x)));
  }, []);

  const meaning = recall === "meaning";
  const providerId = from.startsWith("provider:") ? from.slice("provider:".length) : null;
  const provider = providers.find((p) => p.id === providerId);
  const tag = provider ? from : from === "ollama" ? `ollama:${ollamaUrl.trim()}` : null;
  const listing = tag ? listings[tag] : undefined;
  // A provider's cached listing loads when its tab is first shown; Ollama
  // is asked once per URL, after typing stops. Never again by themselves.
  useEffect(() => {
    if (!meaning || !provider || listings[`provider:${provider.id}`]) return;
    void list(`provider:${provider.id}`, providerList(provider.id, false));
  }, [meaning, provider, listings, list]);
  useEffect(() => {
    const url = ollamaUrl.trim();
    if (!meaning || from !== "ollama" || pick !== "custom" || !url || listings[`ollama:${url}`]) return;
    const t = setTimeout(() => void list(`ollama:${url}`, ollamaList(url)), 400);
    return () => clearTimeout(t);
  }, [meaning, from, pick, ollamaUrl, listings, list]);
  useEffect(() => {
    if (meaning && from === "custom") setRegistering(true);
  }, [meaning, from]);

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
  const locked = cfg.locked ?? [];
  if (locked.length > 0) {
    return (
      <div className="stack">
        <div className="muted">
          Memory is set by the server&apos;s environment (.env): {cfg.backend} backend, {cfg.recall} recall
          {cfg.recall === "meaning" && cfg.model ? `, embeddings model ${cfg.model}` : ""}. Change{" "}
          {locked.map((l) => l.var).join(", ")} there and restart to adjust it.
        </div>
        {onward}
      </div>
    );
  }
  if (cfg.backend !== "builtin" && cfg.backend !== "qdrant") {
    return (
      <div className="stack">
        <div className="muted">Memory uses the {cfg.backend} backend (set in config.toml).</div>
        {onward}
      </div>
    );
  }

  const slot = provider ? from : "ollama";
  const model = models[slot] ?? "";
  const setModel = (v: string) => setModels((m) => ({ ...m, [slot]: v }));
  const preset = cfg.presets.find((p) => p.id === pick);
  const keep = pick === "custom" ? cfg.preset : pick;
  const needsModel = from !== "ollama" || pick === "custom";
  const ready = !meaning || (from !== "custom" && (from !== "ollama" || !!ollamaUrl.trim()) && (from === "ollama" || !!provider) && (!needsModel || !!model.trim()));

  // The listing, models added by hand, and the saved model even when
  // neither has it.
  const added = extra[slot] ?? [];
  const shown = [...added, ...(listing?.models ?? []).filter((m) => !added.some((x) => x.id === m.id))];
  if (model && !shown.some((m) => m.id === model)) shown.unshift(byHand(model));
  const target = provider
    ? { provider_id: provider.id, preset: provider.preset, base_url: provider.base_url, kind: provider.kind }
    : { preset: "ollama", base_url: ollamaUrl.trim(), kind: "openai" as const };
  const picker = tag && (
    <>
      {listing?.busy && !listing.models && <div className="muted small">Listing models…</div>}
      {listing?.error && <div className="error-text small">{listing.error}</div>}
      <ModelPicker
        label="Embeddings model"
        mode="single"
        embeddings
        models={shown}
        sort={embedFirst}
        selected={model ? [model] : []}
        onChange={(ids) => setModel(ids[0] ?? "")}
        onRefresh={() => void list(tag, provider ? providerList(provider.id, true) : ollamaList(ollamaUrl.trim()))}
        refreshing={!!listing?.busy}
        fetchedMs={provider ? listing?.fetched : null}
        custom={{
          test: () => target,
          onSave: (m, was) => {
            setExtra((x) => ({ ...x, [slot]: [...(x[slot] ?? []).filter((y) => y.id !== m.id && y.id !== was), m] }));
            setModel(m.id);
          },
          onRemove: (id) => {
            setExtra((x) => ({ ...x, [slot]: (x[slot] ?? []).filter((y) => y.id !== id) }));
            if (model === id) setModel("");
          },
        }}
      />
    </>
  );

  // A provider registered for memory: memory then uses it.
  const registered = async (s: ProvidersState, saved: { id: string; embeddings: string | null }) => {
    if (!props.providers) setLoaded(s);
    props.onProviders?.(s);
    setErr(null);
    setMsg(null);
    try {
      const c = await pageApi.saveMemoryCfg({
        recall: "meaning",
        source: "provider",
        provider_id: saved.id,
        embeddings_model: saved.embeddings ?? "",
        preset: keep,
      });
      setCfg(c);
      setFrom(`provider:${saved.id}`);
      setModels((m) => ({ ...m, [`provider:${saved.id}`]: saved.embeddings ?? "" }));
      setMsg(c.active_model ? `Using ${c.active_model}.` : "Saved.");
      props.onSaved?.();
    } catch (x) {
      setErr(errText(x));
    }
  };

  return (
    <form
      className="form stack"
      onSubmit={async (e) => {
        e.preventDefault();
        // The registration form saves by its own button.
        if (meaning && from === "custom") return;
        setBusy(true);
        setErr(null);
        setMsg(null);
        try {
          const body: Parameters<typeof pageApi.saveMemoryCfg>[0] = { recall, source: provider ? "provider" : "ollama", preset: keep };
          if (meaning && provider) Object.assign(body, { provider_id: provider.id, embeddings_model: model.trim() });
          if (meaning && from === "ollama")
            Object.assign(body, { embeddings_url: ollamaUrl.trim(), embeddings_model: pick === "custom" ? model.trim() : "" });
          const c = await pageApi.saveMemoryCfg(body);
          setCfg(c);
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
              wrap={providers.length > 1}
              value={from}
              options={[
                ...providers.map((p) => ({ value: `provider:${p.id}` as From, label: p.name })),
                { value: "ollama" as const, label: "Ollama" },
                { value: "custom" as const, label: "Custom" },
              ]}
              onChange={setFrom}
            />
          </div>
          {provider && (
            <>
              <span className="field-hint">
                Uses {provider.name}'s endpoint and key. Many providers offer embeddings models; listings often leave them out, so
                add one by hand when it is missing.
              </span>
              {picker}
            </>
          )}
          {from === "ollama" && (
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
        </>
      )}
      {registering && (
        <div hidden={!(meaning && from === "custom")}>
          {pstate ? (
            <ProviderForm
              state={pstate}
              agents={[]}
              purpose="embeddings"
              saveLabel={props.saveLabel ?? "Save"}
              actions={meaning && from === "custom" ? props.actions : undefined}
              onSaved={(s, saved) => void registered(s, saved)}
            />
          ) : (
            <div className="muted">Loading…</div>
          )}
        </div>
      )}
      {err && <div className="error-text">{err}</div>}
      {!(meaning && from === "custom") && (
        <div className={props.actions ? "row" : "row end"}>
          {props.actions}
          {props.actions && <span className="spacer" />}
          {msg && <span className="muted">{msg}</span>}
          <button className="primary" disabled={busy || !ready}>
            {busy ? "Testing…" : (props.saveLabel ?? "Save")}
          </button>
        </div>
      )}
      {meaning && from === "custom" && msg && <div className="muted">{msg}</div>}
    </form>
  );
}

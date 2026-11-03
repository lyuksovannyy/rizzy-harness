// Memory: what the assistant remembers about you. Proposals from chats wait
// here for a confirm; only confirmed facts are used.
import { useCallback, useEffect, useState } from "react";
import { api } from "../api";
import type { FactKind, MemoryCfg, MemoryFact, Preset } from "../types";

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

/** Admin: smarter recall through an embeddings endpoint (e.g. Ollama). */
export function MemoryCfgForm(props: { onSaved?: () => void; saveLabel?: string }) {
  const [cfg, setCfg] = useState<MemoryCfg | null>(null);
  const [on, setOn] = useState(false);
  const [url, setUrl] = useState("http://localhost:11434/v1");
  const [preset, setPreset] = useState<Preset>("balanced");
  const [model, setModel] = useState("");
  const [busy, setBusy] = useState(false);
  const [msg, setMsg] = useState<string | null>(null);
  const [err, setErr] = useState<string | null>(null);
  useEffect(() => {
    api
      .memoryCfg()
      .then((c) => {
        setCfg(c);
        setOn(!!c.embeddings_url);
        if (c.embeddings_url) setUrl(c.embeddings_url);
        setPreset(c.preset);
        setModel(c.embeddings_model);
      })
      .catch((x) => setErr(String(x)));
  }, []);
  if (!cfg) return err ? <div className="error-text">{err}</div> : <div className="muted">Loading…</div>;
  if (cfg.backend !== "builtin") {
    return <div className="muted">Memory uses the {cfg.backend} backend (set in harness.toml).</div>;
  }
  return (
    <form
      className="form stack"
      onSubmit={async (e) => {
        e.preventDefault();
        setBusy(true);
        setErr(null);
        setMsg(null);
        try {
          const c = await api.saveMemoryCfg({ embeddings_url: on ? url : "", embeddings_model: model, preset });
          setCfg(c);
          setMsg(c.active_model ? `Using ${c.active_model}.` : "Keyword recall only.");
          props.onSaved?.();
        } catch (x) {
          setErr(x instanceof Error ? x.message : String(x));
        } finally {
          setBusy(false);
        }
      }}
    >
      <div className="engines">
        <label className="check">
          <input type="radio" name="recall" checked={!on} onChange={() => setOn(false)} />
          Keywords (no extra service)
        </label>
        <label className="check">
          <input type="radio" name="recall" checked={on} onChange={() => setOn(true)} />
          Meaning (embeddings model)
        </label>
      </div>
      {on && (
        <>
          <label>
            Embeddings endpoint (OpenAI-compatible, e.g. Ollama)
            <input className="mono" value={url} onChange={(e) => setUrl(e.target.value)} />
          </label>
          <div className="presets" role="radiogroup" aria-label="memory preset">
            {cfg.presets.map((p) => (
              <label key={p.id} className="check">
                <input type="radio" name="preset" checked={preset === p.id} onChange={() => setPreset(p.id)} />
                <b>{p.id[0]!.toUpperCase() + p.id.slice(1)}</b>
                <span className="muted small mono">
                  {p.model} · {p.size}
                </span>
              </label>
            ))}
          </div>
          <label>
            Model override (optional)
            <input className="mono" placeholder={cfg.presets.find((p) => p.id === preset)?.model} value={model} onChange={(e) => setModel(e.target.value)} />
          </label>
          <div className="muted small">Sizes are the models' download sizes. Pull the model first, e.g. ollama pull {model || cfg.presets.find((p) => p.id === preset)?.model}.</div>
        </>
      )}
      {err && <div className="error-text">{err}</div>}
      <div className="row end">
        {msg && <span className="muted">{msg}</span>}
        <button className="primary" disabled={busy}>
          {busy ? "Testing…" : (props.saveLabel ?? "Save")}
        </button>
      </div>
    </form>
  );
}

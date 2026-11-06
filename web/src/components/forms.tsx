// Forms shared by onboarding and Settings.
import { Star } from "lucide-react";
import { useEffect, useState } from "react";
import { api } from "../api";
import { browserTimeZone } from "../lib/greeting";
import { countryCode, isTimeZone, loadCities, suggestCities, suggestCountries, suggestTimeZones } from "../lib/geo";
import { Combobox } from "./Combobox";
import type { Prefs, ProviderKind, ProviderModel, ProviderState } from "../types";

const PRESETS: { label: string; kind: ProviderKind; base: string; needsKey: boolean }[] = [
  { label: "OpenAI", kind: "openai", base: "https://api.openai.com/v1", needsKey: true },
  { label: "Anthropic (API key)", kind: "anthropic", base: "https://api.anthropic.com/v1", needsKey: true },
  { label: "DeepSeek", kind: "openai", base: "https://api.deepseek.com", needsKey: true },
  { label: "OpenRouter", kind: "openai", base: "https://openrouter.ai/api/v1", needsKey: true },
  { label: "Ollama on this machine", kind: "openai", base: "http://localhost:11434/v1", needsKey: false },
  { label: "Other OpenAI-compatible", kind: "openai", base: "", needsKey: true },
];

/** Admin: pick a provider, test it (lists models), choose models, save. */
export function ProviderForm(props: { onSaved: (p: ProviderState) => void; saveLabel?: string }) {
  const [current, setCurrent] = useState<ProviderState | null>(null);
  const [preset, setPreset] = useState(0);
  const [base, setBase] = useState(PRESETS[0]!.base);
  const [key, setKey] = useState("");
  const [models, setModels] = useState<string[] | null>(null);
  const [chosen, setChosen] = useState<Record<string, ProviderModel>>({});
  const [def, setDef] = useState("");
  const [msg, setMsg] = useState<string | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const p = PRESETS[preset]!;

  useEffect(() => {
    api
      .provider()
      .then((s) => {
        setCurrent(s);
        if (s.configured) {
          const i = PRESETS.findIndex((x) => x.kind === s.kind && x.base === s.base_url);
          setPreset(i >= 0 ? i : PRESETS.length - 1);
          setBase(s.base_url);
          setDef(s.model);
          setChosen(Object.fromEntries(s.models.map((m) => [m.id, m])));
        }
      })
      .catch((e) => setErr(String(e)));
  }, []);

  const keyNote =
    current?.configured && current.kind === p.kind
      ? current.key.startsWith("env:")
        ? `· read from ${current.key.slice(4)}; leave empty to keep`
        : "· a key is stored; leave empty to keep"
      : null;

  const test = async () => {
    setBusy(true);
    setErr(null);
    setMsg(null);
    try {
      const r = await api.testProvider({ kind: p.kind, base_url: base, api_key: key || undefined });
      if (!r.ok) throw new Error(r.error ?? "test failed");
      setModels(r.models ?? []);
      setMsg(`Connected. ${r.models?.length ?? 0} models available.`);
      if (!def && r.models?.length) {
        setDef(r.models[0]!);
        setChosen({ [r.models[0]!]: { id: r.models[0]!, effort: false, vision: false } });
      }
    } catch (x) {
      setModels(null);
      setErr(x instanceof Error ? x.message : String(x));
    } finally {
      setBusy(false);
    }
  };

  const save = async () => {
    setBusy(true);
    setErr(null);
    try {
      const list = Object.values(chosen);
      if (def && !chosen[def]) list.unshift({ id: def, effort: false, vision: false });
      const s = await api.saveProvider({
        kind: p.kind,
        base_url: base,
        api_key: key || undefined,
        model: def,
        models: list,
      });
      setKey("");
      setCurrent(s);
      props.onSaved(s);
    } catch (x) {
      setErr(x instanceof Error ? x.message : String(x));
    } finally {
      setBusy(false);
    }
  };

  const toggle = (id: string, on: boolean) =>
    setChosen((c) => {
      const n = { ...c };
      if (on) n[id] = n[id] ?? { id, effort: false, vision: false };
      else delete n[id];
      return n;
    });

  return (
    <div className="form stack">
      <label>
        Provider
        <select
          value={preset}
          onChange={(e) => {
            const i = +e.target.value;
            setPreset(i);
            setBase(PRESETS[i]!.base);
            setModels(null);
            setMsg(null);
          }}
        >
          {PRESETS.map((x, i) => (
            <option key={x.label} value={i}>
              {x.label}
            </option>
          ))}
        </select>
      </label>
      <label>
        API endpoint
        <input className="mono" value={base} onChange={(e) => setBase(e.target.value)} placeholder="https://…/v1" />
      </label>
      {p.needsKey && (
        <label>
          <span>
            API key <span className="muted">{keyNote ?? "· stored only on this server (0600)"}</span>
          </span>
          <input
            type="password"
            autoComplete="off"
            value={key}
            onChange={(e) => setKey(e.target.value)}
            placeholder={keyNote ? "••••••••" : "sk-…"}
          />
        </label>
      )}
      {p.kind === "anthropic" && (
        <p className="muted small">Anthropic is supported with an API key only. Claude subscription logins cannot be used by third-party tools.</p>
      )}
      <div className="row">
        <button type="button" onClick={test} disabled={busy || !base.trim()}>
          Test connection
        </button>
        {msg && <span className="muted">{msg}</span>}
      </div>
      {models && models.length > 0 && (
        <div className="model-pick">
          <div className="muted small">Pick the models to offer; the starred one is the default.</div>
          <ul>
            {models.map((m) => (
              <li key={m}>
                <label className="check">
                  <input type="checkbox" checked={!!chosen[m]} onChange={(e) => toggle(m, e.target.checked)} />
                  <span className="mono">{m}</span>
                </label>
                <button
                  type="button"
                  className={`link star ${def === m ? "on" : ""}`}
                  aria-label={`make ${m} the default`}
                  onClick={() => {
                    setDef(m);
                    toggle(m, true);
                  }}
                >
                  <Star size={16} fill={def === m ? "currentColor" : "none"} />
                </button>
                {chosen[m] && (
                  <label className="check small">
                    <input
                      type="checkbox"
                      checked={chosen[m]!.effort}
                      onChange={(e) => setChosen((c) => ({ ...c, [m]: { ...c[m]!, effort: e.target.checked } }))}
                    />
                    effort
                  </label>
                )}
              </li>
            ))}
          </ul>
        </div>
      )}
      {err && <div className="error-text">{err}</div>}
      <div className="row end">
        <button className="primary" type="button" onClick={save} disabled={busy || !models || !def}>
          {props.saveLabel ?? "Save provider"}
        </button>
      </div>
    </div>
  );
}

/** Use the built-in demo provider (no AI) for now. */
export async function chooseDemoProvider(): Promise<ProviderState> {
  return api.saveProvider({ kind: "mock", models: [] });
}

export function ProfileFields(props: { prefs: Prefs; onChange: (p: Prefs) => void; username: string }) {
  const { prefs, onChange } = props;
  const [cities, setCities] = useState<[string, string][] | null>(null);
  const set = (k: keyof Prefs, v: string) => onChange({ ...prefs, [k]: v.trim() ? v : null });
  const tz = prefs.timezone ?? "";
  const tzBad = tz.trim() !== "" && !isTimeZone(tz.trim());
  const device = browserTimeZone();
  const cc = countryCode(prefs.country);
  return (
    <div className="form stack">
      <label>
        What should I call you?
        <input
          value={prefs.display_name ?? ""}
          placeholder={props.username}
          onChange={(e) => set("display_name", e.target.value)}
        />
      </label>
      <div className="field">
        <span>
          Time zone <span className="muted">(optional)</span>
        </span>
        <Combobox
          label="Time zone"
          value={tz}
          onChange={(v) => set("timezone", v)}
          suggest={suggestTimeZones}
          placeholder="e.g. Europe/Riga"
          invalid={tzBad}
        />
        {tzBad && <span className="error-text small">Pick a time zone from the list.</span>}
        {!tz && (
          <span className="muted small">
            Empty: the AI is not told your local time.{" "}
            <button type="button" className="link" onClick={() => set("timezone", device)}>
              Use this device's ({device})
            </button>
          </span>
        )}
      </div>
      <div className="row">
        <div className="field grow">
          <span>
            Country <span className="muted">(optional)</span>
          </span>
          <Combobox
            label="Country"
            value={prefs.country ?? ""}
            onChange={(v) => set("country", v)}
            suggest={suggestCountries}
          />
        </div>
        <div className="field grow">
          <span>
            City <span className="muted">(optional)</span>
          </span>
          <Combobox
            label="City"
            value={prefs.city ?? ""}
            onChange={(v) => set("city", v)}
            onFirstFocus={() => void loadCities().then(setCities)}
            suggest={(q) => (cities ? suggestCities(cities, q, cc) : [])}
          />
        </div>
      </div>
      <p className="muted small">
        Country and city only tell the AI where to look for local things (transit, shops, news). City names: GeoNames
        (CC BY 4.0).
      </p>
    </div>
  );
}

const ENGINES: { label: string; url: string }[] = [
  { label: "DuckDuckGo", url: "https://duckduckgo.com/?q={q}" },
  { label: "Google", url: "https://www.google.com/search?q={q}" },
  { label: "Brave Search", url: "https://search.brave.com/search?q={q}" },
  { label: "Kagi", url: "https://kagi.com/search?q={q}" },
  { label: "Startpage", url: "https://www.startpage.com/do/search?q={q}" },
  { label: "Bing", url: "https://www.bing.com/search?q={q}" },
];

export function SearchEngineFields(props: { prefs: Prefs; onChange: (p: Prefs) => void }) {
  const { prefs, onChange } = props;
  const known = ENGINES.find((e) => e.url === prefs.search_url);
  return (
    <div className="form stack">
      <div className="engines" role="radiogroup" aria-label="search engine">
        {ENGINES.map((e) => (
          <label key={e.url} className="check">
            <input
              type="radio"
              name="engine"
              checked={prefs.search_url === e.url}
              onChange={() => onChange({ ...prefs, search_url: e.url })}
            />
            {e.label}
          </label>
        ))}
        <label className="check">
          <input type="radio" name="engine" checked={!known} onChange={() => onChange({ ...prefs, search_url: "https://" })} />
          Custom
        </label>
      </div>
      {!known && (
        <label>
          <span>
            Search URL <span className="muted">(use {"{q}"} for the query)</span>
          </span>
          <input className="mono" value={prefs.search_url} onChange={(e) => onChange({ ...prefs, search_url: e.target.value })} />
        </label>
      )}
    </div>
  );
}

export const ACCENTS = ["blue", "violet", "green", "orange", "pink", "gray"] as const;

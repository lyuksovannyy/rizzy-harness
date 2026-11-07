// Forms shared by onboarding and Settings.
import { Plus, RotateCcw, Trash2 } from "lucide-react";
import { useEffect, useId, useState } from "react";
import { browserTimeZone } from "../lib/greeting";
import { countryCode, countryFlag, isTimeZone, loadCities, suggestCities, suggestCountries, suggestTimeZones } from "../lib/geo";
import { type ComboAction, Combobox } from "./Combobox";
import {
  abilities,
  allEngines,
  type CustomEngine,
  DEFAULT_IMAGE_SEARCH,
  type Engine,
  engineOrigin,
  type SearchKind,
  supports,
  urlOrigin,
} from "../lib/engines";
import { EngineIcon } from "./EngineIcon";
import { Select, type SelectOption } from "./Select";
import type { Prefs } from "../types";

export function ProfileFields(props: { prefs: Prefs; onChange: (p: Prefs) => void; username: string }) {
  const { prefs, onChange } = props;
  const id = useId();
  const [cities, setCities] = useState<[string, string][] | null>(null);
  const set = (k: keyof Prefs, v: string) => onChange({ ...prefs, [k]: v.trim() ? v : null });
  const tz = prefs.timezone ?? "";
  const tzBad = tz.trim() !== "" && !isTimeZone(tz.trim());
  const device = browserTimeZone();
  const cc = countryCode(prefs.country);
  const tzActions: ComboAction[] =
    tz.trim() && !tzBad && tz.trim() !== device
      ? [{ label: `Reset to this device's time zone (${device})`, icon: <RotateCcw size={14} />, onClick: () => set("timezone", device) }]
      : [];
  return (
    <div className="form stack">
      <div className="field">
        <label className="field-label" htmlFor={`${id}-name`}>
          Name <Optional />
        </label>
        <input
          id={`${id}-name`}
          value={prefs.display_name ?? ""}
          placeholder={props.username}
          onChange={(e) => set("display_name", e.target.value)}
        />
        <span className="field-hint">What the assistant calls you.</span>
      </div>
      <div className="field">
        <label className="field-label" htmlFor={`${id}-tz`}>
          Time zone <Optional />
        </label>
        <Combobox
          id={`${id}-tz`}
          label="Time zone"
          value={tz}
          onChange={(v) => set("timezone", v)}
          suggest={suggestTimeZones}
          placeholder="e.g. Europe/Riga"
          invalid={tzBad}
          actions={tzActions}
        />
        {tzBad && <span className="field-hint error-text">Pick a time zone from the list.</span>}
        {!tz && (
          <span className="field-hint">
            Empty: the AI is not told your local time.{" "}
            <button type="button" className="link" onClick={() => set("timezone", device)}>
              Use this device's ({device})
            </button>
          </span>
        )}
      </div>
      <div className="row">
        <div className="field grow">
          <label className="field-label" htmlFor={`${id}-country`}>
            Country <Optional />
          </label>
          <Combobox
            id={`${id}-country`}
            label="Country"
            value={prefs.country ?? ""}
            onChange={(v) => set("country", v)}
            suggest={suggestCountries}
            icon={cc ? countryFlag(cc) : undefined}
          />
        </div>
        <div className="field grow">
          <label className="field-label" htmlFor={`${id}-city`}>
            City <Optional />
          </label>
          <Combobox
            id={`${id}-city`}
            label="City"
            value={prefs.city ?? ""}
            onChange={(v) => set("city", v)}
            onFirstFocus={() => void loadCities().then(setCities)}
            suggest={(q) => (cities ? suggestCities(cities, q, cc) : [])}
            footer={
              <>
                City names:{" "}
                <a href="https://www.geonames.org/" target="_blank" rel="noreferrer">
                  GeoNames
                </a>{" "}
                (CC BY 4.0)
              </>
            }
          />
        </div>
      </div>
    </div>
  );
}

function Optional() {
  return <span className="optional">(optional)</span>;
}

/** Options for an engine selector: every engine able to do `kind`. */
function engineOptions(engines: Engine[], kind: SearchKind): SelectOption[] {
  return engines
    .filter((e) => supports(e, kind))
    .map((e) => ({
      value: (kind === "text" ? e.text : e.image)!,
      label: e.name,
      hint: abilities(e),
      icon: <EngineIcon origin={engineOrigin(e)} />,
    }));
}

/** `value` once it has stopped changing for `ms`. */
function useSettled<T>(value: T, ms: number): T {
  const [settled, setSettled] = useState(value);
  useEffect(() => {
    const t = setTimeout(() => setSettled(value), ms);
    return () => clearTimeout(t);
  }, [value, ms]);
  return settled;
}

/** A custom engine's favicon, looked up once its URL stops changing. */
function CustomEngineIcon(props: { origin: string | null }) {
  return <EngineIcon origin={useSettled(props.origin, 600)} />;
}

function CustomEngines(props: { prefs: Prefs; onChange: (p: Prefs) => void }) {
  const list = props.prefs.custom_engines ?? [];
  const put = (i: number, c: CustomEngine | null) => {
    const next = list.slice();
    if (c) next[i] = c;
    else next.splice(i, 1);
    props.onChange({ ...props.prefs, custom_engines: next });
  };
  return (
    <div className="stack">
      <span>
        Your own engines <span className="muted">(use {"{q}"} where the query goes)</span>
      </span>
      {list.map((c, i) => (
        <div key={i} className="custom-engine stack" aria-label={`custom engine ${i + 1}`}>
          <div className="row">
            <CustomEngineIcon origin={urlOrigin(c.text_url) ?? urlOrigin(c.image_url)} />
            <input
              aria-label="Engine name"
              placeholder="Name"
              value={c.name}
              onChange={(e) => put(i, { ...c, name: e.target.value })}
            />
            <label className="check">
              <input
                type="checkbox"
                checked={c.text_url != null}
                onChange={(e) => put(i, { ...c, text_url: e.target.checked ? "https://" : null })}
              />
              Text
            </label>
            <label className="check">
              <input
                type="checkbox"
                checked={c.image_url != null}
                onChange={(e) => put(i, { ...c, image_url: e.target.checked ? "https://" : null })}
              />
              Image
            </label>
            <button type="button" className="icon" aria-label={`Remove ${c.name || "engine"}`} onClick={() => put(i, null)}>
              <Trash2 size={16} />
            </button>
          </div>
          {c.text_url != null && (
            <input
              className="mono"
              aria-label="Text search URL"
              placeholder="https://example.com/search?q={q}"
              value={c.text_url}
              onChange={(e) => put(i, { ...c, text_url: e.target.value })}
            />
          )}
          {c.image_url != null && (
            <input
              className="mono"
              aria-label="Image search URL"
              placeholder="https://example.com/images?q={q}"
              value={c.image_url}
              onChange={(e) => put(i, { ...c, image_url: e.target.value })}
            />
          )}
        </div>
      ))}
      <div>
        <button
          type="button"
          onClick={() => props.onChange({ ...props.prefs, custom_engines: [...list, { name: "", text_url: "https://" }] })}
        >
          <Plus size={14} /> Add a search engine
        </button>
      </div>
    </div>
  );
}

export function SearchEngineFields(props: { prefs: Prefs; onChange: (p: Prefs) => void }) {
  const { prefs, onChange } = props;
  const engines = allEngines(prefs.custom_engines);
  const imageUrl = prefs.image_search_url ?? DEFAULT_IMAGE_SEARCH;
  return (
    <div className="form stack">
      <div className="row wrap">
        <div className="field">
          <span>Web search</span>
          <Select
            label="Web search engine"
            value={prefs.search_url}
            options={engineOptions(engines, "text")}
            onChange={(v) => onChange({ ...prefs, search_url: v })}
          />
        </div>
        <div className="field">
          <span>Image search</span>
          <Select
            label="Image search engine"
            value={imageUrl}
            options={engineOptions(engines, "image")}
            onChange={(v) => onChange({ ...prefs, image_search_url: v })}
          />
        </div>
      </div>
      <p className="field-hint">
        In Search mode you can switch between web and image search, pick another engine, or start the query with its
        name, like <code>@yandex red panda</code>.
      </p>
      <CustomEngines prefs={prefs} onChange={onChange} />
    </div>
  );
}

export const ACCENTS = ["blue", "violet", "green", "orange", "pink", "gray"] as const;
/** The theme choices; "system" is stored as null. */
export const THEMES = ["system", "light", "dark"] as const;
/** The home backgrounds; "animated" is stored as null. */
export const BACKDROPS = ["animated", "still", "off"] as const;

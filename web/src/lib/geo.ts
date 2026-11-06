// Suggestions for the profile's time zone, country and city fields. Time
// zones and country names come from the browser (Intl); cities from
// /cities.json (GeoNames, CC BY 4.0), loaded on first use.

export interface Suggestion {
  value: string;
  label: string;
  hint?: string;
}

/** Codes Intl names that are not countries (groups, pseudo-locales, retired codes). */
const NOT_COUNTRIES = new Set([
  "EU", "EZ", "UN", "QO", "XA", "XB", "ZZ",
  "AN", "BU", "CS", "DD", "DY", "FX", "HV", "NH", "NT", "RH", "SU", "TP", "UK", "VD", "YD", "YU", "ZR",
]);

/** Fold case and accents: "Zürich" matches "zurich". */
export function fold(s: string): string {
  return s.normalize("NFD").replace(/\p{M}/gu, "").toLowerCase();
}

let countryCache: { code: string; name: string }[] | null = null;

export function countries(): { code: string; name: string }[] {
  if (countryCache) return countryCache;
  const out: { code: string; name: string }[] = [];
  const seen = new Set<string>();
  try {
    const dn = new Intl.DisplayNames(["en"], { type: "region" });
    for (let a = 65; a < 91; a++) {
      for (let b = 65; b < 91; b++) {
        const code = String.fromCharCode(a, b);
        if (NOT_COUNTRIES.has(code)) continue;
        let name: string | undefined;
        try {
          name = dn.of(code);
        } catch {
          continue;
        }
        if (!name || name === code || seen.has(name)) continue;
        seen.add(name);
        out.push({ code, name });
      }
    }
  } catch {
    // Very old browser: no suggestions, free text still works.
  }
  out.sort((x, y) => x.name.localeCompare(y.name));
  countryCache = out;
  return out;
}

/** The ISO code for a country name the user picked or typed. */
export function countryCode(name: string | null | undefined): string | null {
  if (!name) return null;
  const f = fold(name.trim());
  return countries().find((c) => fold(c.name) === f || c.code.toLowerCase() === f)?.code ?? null;
}

export function timeZones(): string[] {
  try {
    return (Intl as unknown as { supportedValuesOf?: (k: string) => string[] }).supportedValuesOf?.("timeZone") ?? [];
  } catch {
    return [];
  }
}

export function isTimeZone(tz: string): boolean {
  try {
    new Intl.DateTimeFormat("en", { timeZone: tz });
    return true;
  } catch {
    return false;
  }
}

/** "UTC+02:00" for a zone right now. */
export function utcOffset(tz: string, now = new Date()): string {
  try {
    const p = new Intl.DateTimeFormat("en", { timeZone: tz, timeZoneName: "longOffset" })
      .formatToParts(now)
      .find((x) => x.type === "timeZoneName")?.value;
    return p === "GMT" || p === "GMT+00:00" ? "UTC" : (p ?? "").replace("GMT", "UTC");
  } catch {
    return "";
  }
}

/**
 * Rank `items` against `query`: whole-text prefix first, then a word that
 * starts with the query, then anywhere. Accents and case are ignored;
 * `_` and `/` count as word breaks (time zones).
 */
export function rank<T>(items: T[], query: string, text: (t: T) => string, limit = 8): T[] {
  const q = fold(query.trim()).replace(/[_/]+/g, " ");
  if (!q) return items.slice(0, limit);
  const scored: [number, number, T][] = [];
  items.forEach((it, i) => {
    const t = fold(text(it)).replace(/[_/]+/g, " ");
    let s = -1;
    if (t.startsWith(q)) s = 0;
    else if (t.includes(" " + q) || t.includes("-" + q) || t.includes("(" + q)) s = 1;
    else if (t.includes(q)) s = 2;
    if (s >= 0) scored.push([s, i, it]);
  });
  scored.sort((a, b) => a[0] - b[0] || a[1] - b[1]);
  return scored.slice(0, limit).map((x) => x[2]);
}

export function suggestTimeZones(query: string): Suggestion[] {
  return rank(timeZones(), query, (z) => z).map((z) => ({
    value: z,
    label: z.replace(/_/g, " "),
    hint: utcOffset(z),
  }));
}

export function suggestCountries(query: string): Suggestion[] {
  return rank(countries(), query, (c) => c.name).map((c) => ({ value: c.name, label: c.name, hint: c.code }));
}

type City = [string, string];
let cityLoad: Promise<City[]> | null = null;

export function loadCities(): Promise<City[]> {
  cityLoad ??= fetch("/cities.json")
    .then((r) => (r.ok ? r.json() : { cities: [] }))
    .then((d: { cities?: City[] }) => d.cities ?? [])
    .catch(() => {
      cityLoad = null;
      return [];
    });
  return cityLoad;
}

/** Cities (largest first) in the chosen country, or anywhere if none. */
export function suggestCities(all: City[], query: string, country: string | null): Suggestion[] {
  const pool = country ? all.filter((c) => c[1] === country) : all;
  const names = new Map(countries().map((c) => [c.code, c.name]));
  return rank(pool, query, (c) => c[0]).map((c) => ({
    value: c[0],
    label: c[0],
    hint: country ? undefined : (names.get(c[1]) ?? c[1]),
  }));
}

// Search engines for Search mode: built-ins plus the user's own. An engine's
// abilities are the URLs it has (text, image or both); `{q}` is the query.

export type SearchKind = "text" | "image";

/** Same as the server's default for `image_search_url`. */
export const DEFAULT_IMAGE_SEARCH = "https://duckduckgo.com/?q={q}&iax=images&ia=images";

export interface Engine {
  /** Mention handle: `@google cats`. */
  id: string;
  name: string;
  text?: string;
  image?: string;
  custom?: boolean;
}

export interface CustomEngine {
  name: string;
  text_url?: string | null;
  image_url?: string | null;
}

export const BUILTIN_ENGINES: Engine[] = [
  { id: "google", name: "Google", text: "https://www.google.com/search?q={q}", image: "https://www.google.com/search?udm=2&q={q}" },
  { id: "bing", name: "Bing", text: "https://www.bing.com/search?q={q}", image: "https://www.bing.com/images/search?q={q}" },
  { id: "duckduckgo", name: "DuckDuckGo", text: "https://duckduckgo.com/?q={q}", image: "https://duckduckgo.com/?q={q}&iax=images&ia=images" },
  { id: "yandex", name: "Yandex", text: "https://yandex.com/search/?text={q}", image: "https://yandex.com/images/search?text={q}" },
  { id: "brave", name: "Brave Search", text: "https://search.brave.com/search?q={q}", image: "https://search.brave.com/images?q={q}" },
  { id: "kagi", name: "Kagi", text: "https://kagi.com/search?q={q}", image: "https://kagi.com/images?q={q}" },
  { id: "startpage", name: "Startpage", text: "https://www.startpage.com/do/search?q={q}", image: "https://www.startpage.com/do/search?cat=images&query={q}" },
  { id: "ecosia", name: "Ecosia", text: "https://www.ecosia.org/search?q={q}", image: "https://www.ecosia.org/images?q={q}" },
  { id: "wikipedia", name: "Wikipedia", text: "https://en.wikipedia.org/w/index.php?search={q}" },
  { id: "unsplash", name: "Unsplash", image: "https://unsplash.com/s/photos/{q}" },
];

/** Mention handle for a name: lowercase letters and digits. */
export function handle(name: string): string {
  return name.toLowerCase().replace(/[^a-z0-9]+/g, "") || "engine";
}

/** Built-ins plus custom engines (custom handles never shadow a built-in). */
export function allEngines(custom: CustomEngine[] | undefined): Engine[] {
  const taken = new Set(BUILTIN_ENGINES.map((e) => e.id));
  const mine: Engine[] = (custom ?? []).map((c) => {
    let id = handle(c.name);
    while (taken.has(id)) id += "2";
    taken.add(id);
    return { id, name: c.name, text: c.text_url || undefined, image: c.image_url || undefined, custom: true };
  });
  return [...BUILTIN_ENGINES, ...mine];
}

export function abilities(e: Engine): string {
  return e.text && e.image ? "text & image" : e.image ? "image" : "text";
}

export function supports(e: Engine, kind: SearchKind): boolean {
  return Boolean(kind === "text" ? e.text : e.image);
}

/** The engine whose URL for `kind` is `url`, if any. */
export function engineByUrl(engines: Engine[], url: string, kind: SearchKind): Engine | undefined {
  return engines.find((e) => (kind === "text" ? e.text : e.image) === url);
}

/**
 * A leading `@engine` picks the engine for this search: returns the engine
 * and the query without the mention. Unknown handles are left in the query.
 */
export function engineMention(text: string, engines: Engine[]): { engine: Engine; query: string } | null {
  const m = /^@([a-z0-9]+)(?:\s+|$)/i.exec(text.trimStart());
  if (!m) return null;
  const engine = engines.find((e) => e.id === m[1]!.toLowerCase());
  return engine ? { engine, query: text.trimStart().slice(m[0].length) } : null;
}

export function searchUrl(template: string, query: string): string {
  return template.replaceAll("{q}", encodeURIComponent(query.trim()));
}

// Model ids are `<provider_id>::<raw>`. Ids saved before providers had
// prefixes (old sessions, preferences) are raw: they mean the first
// provider listing that id.
import type { ModelInfo, Prefs } from "../types";

/** The model `id` names, prefixed or legacy raw. */
export function resolveModel(models: ModelInfo[], id: string | null | undefined): ModelInfo | undefined {
  if (!id) return undefined;
  return models.find((m) => m.id === id) ?? (id.includes("::") ? undefined : models.find((m) => m.raw === id));
}

/** The raw id, and the provider when there are several to tell apart. */
export function modelLabel(m: ModelInfo, models: ModelInfo[]): string {
  return models.some((x) => x.provider_id !== m.provider_id) ? `${m.raw} · ${m.provider}` : m.raw;
}

/** The prefs to save: a default model that no longer exists (its provider
 * was removed) goes as the server default the select already shows, which
 * the server would otherwise refuse as unknown. */
export function savablePrefs(p: Prefs, models: ModelInfo[]): Prefs {
  if (!p.default_model || !models.length || resolveModel(models, p.default_model)) return p;
  return { ...p, default_model: null };
}

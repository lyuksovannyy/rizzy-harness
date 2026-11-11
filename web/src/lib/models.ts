// Model ids are `<provider_id>::<raw>`. The lookups the first screen needs;
// the plan a form edits and the rest are in modelPlan.ts (lazy pages only).
import type { ModelInfo } from "../types";

/** The model `id` names. */
export function resolveModel(models: ModelInfo[], id: string | null | undefined): ModelInfo | undefined {
  if (!id) return undefined;
  return models.find((m) => m.id === id);
}

/** The raw id of a prefixed model, for short labels. */
export function rawModel(id: string): string {
  const i = id.indexOf("::");
  return i < 0 ? id : id.slice(i + 2) || id;
}

/** The raw id, and the provider when there are several to tell apart. */
export function modelLabel(m: ModelInfo, models: ModelInfo[]): string {
  const s = models.some((x) => x.provider_id !== m.provider_id) ? `${m.raw} · ${m.provider}` : m.raw;
  return m.unavailable ? `${s} (unavailable)` : s;
}

/** The models to offer in a picker: listed ones, plus `current` when it is
 * not (so a select never loses the value it shows). */
export const offeredModels = (models: ModelInfo[], current?: string | null) =>
  models.filter((m) => !m.unavailable || m.id === current);

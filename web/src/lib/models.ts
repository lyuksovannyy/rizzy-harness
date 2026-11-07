// Model ids are `<provider_id>::<raw>`.
import type { AgentChain, ModelInfo, Prefs, ProviderModel } from "../types";

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

/** What a fallback note says: who answered and why; the main model and
 * the provider's error on hover. */
export function fallbackNote(n: { model: string; from: string; reason?: string | null }): { text: string; title: string } {
  const failed = !!n.reason && n.reason !== "unavailable";
  return {
    text: `Answered by ${rawModel(n.model)} — the main model ${failed ? "failed" : "was unavailable"}`,
    title: [`Main model: ${rawModel(n.from)}`, failed && n.reason].filter(Boolean).join("\n"),
  };
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

/** The prefs to save: a default model that no longer exists (its provider
 * was removed) goes as the server default the select already shows, which
 * the server would otherwise refuse as unknown. */
export function savablePrefs(p: Prefs, models: ModelInfo[]): Prefs {
  if (!p.default_model || !models.length || resolveModel(models, p.default_model)) return p;
  return { ...p, default_model: null };
}

/** "embedding" in the id or name first, then "embed", then the rest. */
export function embedRank(m: { id: string; name: string | null }): number {
  const s = `${m.id} ${m.name ?? ""}`.toLowerCase();
  return s.includes("embedding") ? 0 : s.includes("embed") ? 1 : 2;
}

export const embedFirst = (a: ProviderModel, b: ProviderModel) => embedRank(a) - embedRank(b) || a.id.localeCompare(b.id);

// Agents and their models while a form edits them. An agent has one main
// model and, in order, fallbacks; one model may serve many agents. Ids are
// the form's own: prefixed, or `::raw` for a provider whose prefix is not
// settled yet (no real prefix is empty).

/** One agent's models; `model` null: none chosen yet. */
export interface Chain {
  model: string | null;
  fallbacks: string[];
}
export type Plan = Record<string, Chain>;

/** As many fallbacks as the server keeps for one agent. */
export const MAX_FALLBACKS = 8;

const NONE: Chain = { model: null, fallbacks: [] };
const chain = (p: Plan, agent: string): Chain => p[agent] ?? NONE;

/** Makes `id` the agent's main model (null: none); it leaves its fallbacks. */
export function setMain(p: Plan, agent: string, id: string | null): Plan {
  return { ...p, [agent]: { model: id, fallbacks: chain(p, agent).fallbacks.filter((f) => f !== id) } };
}

/** Adds `id` at the end of the agent's fallbacks, or takes it out. The main
 * model made a fallback is no longer the main one. */
export function toggleFallback(p: Plan, agent: string, id: string): Plan {
  const c = chain(p, agent);
  if (c.fallbacks.includes(id)) return { ...p, [agent]: { ...c, fallbacks: c.fallbacks.filter((f) => f !== id) } };
  return { ...p, [agent]: { model: c.model === id ? null : c.model, fallbacks: [...c.fallbacks, id] } };
}

/** Moves fallback `i` one place earlier (-1) or later (1). */
export function moveFallback(p: Plan, agent: string, i: number, by: -1 | 1): Plan {
  const f = [...chain(p, agent).fallbacks];
  const j = i + by;
  if (i < 0 || j < 0 || i >= f.length || j >= f.length) return p;
  [f[i], f[j]] = [f[j]!, f[i]!];
  return { ...p, [agent]: { ...chain(p, agent), fallbacks: f } };
}

/** Takes every id `keep` refuses out of every agent. With `promote`, an
 * agent whose main model goes gets its first remaining fallback instead, as
 * the server repairs a chain. */
export function dropModels(p: Plan, keep: (id: string) => boolean, promote = false): Plan {
  const out: Plan = {};
  for (const [a, c] of Object.entries(p)) {
    const fallbacks = c.fallbacks.filter(keep);
    const lost = c.model !== null && !keep(c.model);
    out[a] =
      lost && promote && fallbacks.length
        ? { model: fallbacks[0]!, fallbacks: fallbacks.slice(1) }
        : { model: c.model !== null && !lost ? c.model : null, fallbacks };
  }
  return out;
}

/** Renames `from` to `to` everywhere (a custom model's id edited). */
export function renameModel(p: Plan, from: string, to: string): Plan {
  const r = (id: string) => (id === from ? to : id);
  const out: Plan = {};
  for (const [a, c] of Object.entries(p)) out[a] = { model: c.model === null ? null : r(c.model), fallbacks: c.fallbacks.map(r) };
  return out;
}

/** What `id` does for each agent that uses it. */
export function rolesOf(p: Plan, id: string): { agent: string; role: "main" | "fallback" }[] {
  const out: { agent: string; role: "main" | "fallback" }[] = [];
  for (const [a, c] of Object.entries(p)) {
    if (c.model === id) out.push({ agent: a, role: "main" });
    else if (c.fallbacks.includes(id)) out.push({ agent: a, role: "fallback" });
  }
  return out;
}

/** Why a plan cannot be saved yet, as sentences; none: it can. `ids` are
 * the chosen models, each of which must serve some agent. */
export function planProblems(p: Plan, ids: string[], agents: { id: string; name: string }[], label: (id: string) => string = (x) => x): string[] {
  const out: string[] = [];
  const used = new Set(agents.flatMap((a) => [chain(p, a.id).model, ...chain(p, a.id).fallbacks]));
  const loose = ids.filter((id) => !used.has(id));
  if (loose.length) out.push(`Assign ${loose.length === 1 ? "this model" : "these models"} to an agent: ${loose.map(label).join(", ")}.`);
  for (const a of agents) {
    const c = chain(p, a.id);
    if (c.fallbacks.length > MAX_FALLBACKS) out.push(`${a.name} has more than ${MAX_FALLBACKS} fallbacks.`);
    if (c.model) continue;
    if (a.id === "general") out.push("General needs a main model: it is the default for everyone.");
    else if (c.fallbacks.length) out.push(`${a.name} has fallbacks but no main model.`);
  }
  return out;
}

/** The plan a form starts from: the saved agents, General on the server
 * default model when it has no chain of its own, and `local`'s models (one
 * provider's prefix) as `::raw`. Models `drop` names are left out. */
export function planFrom(
  saved: Record<string, AgentChain>,
  defaultModel: string | null,
  local: string | null,
  drop: (id: string) => boolean = () => false,
): Plan {
  const own = (id: string) => (local && id.startsWith(`${local}::`) ? id.slice(local.length) : id);
  const p: Plan = {};
  for (const [a, c] of Object.entries(saved)) p[a] = { model: c.model, fallbacks: [...c.fallbacks] };
  if (!p.general && defaultModel) p.general = { model: defaultModel, fallbacks: [] };
  const kept = dropModels(p, (id) => !drop(id));
  const out: Plan = {};
  for (const [a, c] of Object.entries(kept)) out[a] = { model: c.model && own(c.model), fallbacks: c.fallbacks.map(own) };
  return out;
}

/** `::raw` as `<prefix>::raw`; other ids as they are. */
export const realId = (prefix: string, id: string) => (id.startsWith("::") ? `${prefix}${id}` : id);

/** The PUT /api/admin/agent-models body that turns `saved` into `p`, for
 * the agents in `agents`: changed ones only, null for one left without
 * models. */
export function planChanges(saved: Record<string, AgentChain>, p: Plan, prefix: string, agents: string[]): Record<string, AgentChain | null> {
  const out: Record<string, AgentChain | null> = {};
  for (const a of agents) {
    const c = p[a];
    const next = c?.model ? { model: realId(prefix, c.model), fallbacks: c.fallbacks.map((f) => realId(prefix, f)) } : null;
    const was = saved[a] ?? null;
    if (JSON.stringify(next) !== JSON.stringify(was && { model: was.model, fallbacks: was.fallbacks })) out[a] = next;
  }
  return out;
}

/** "3 h ago" for a time in ms; "just now" under a minute. */
export function ago(ms: number, now = Date.now()): string {
  const s = Math.max(0, Math.round((now - ms) / 1000));
  if (s < 60) return "just now";
  if (s < 3600) return `${Math.floor(s / 60)} min ago`;
  if (s < 86_400) return `${Math.floor(s / 3600)} h ago`;
  const d = Math.floor(s / 86_400);
  return `${d} ${d === 1 ? "day" : "days"} ago`;
}

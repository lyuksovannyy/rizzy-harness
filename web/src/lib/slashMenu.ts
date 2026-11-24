// The composer's `/` menu (X.65): its entries and the skills' list, loaded on
// the first `/` of a chat, so the first load does not carry them. A skill's
// name and description are text from a repository somebody else controls:
// the page keeps what fits the shape the server promises and shows the rest
// as text only (showHidden), never as markup or a link.
import { call } from "../api";
import { showHidden } from "../md/hidden";

export interface SlashEntry {
  name: string;
  description: string;
  /** What may follow the name (`[focus]`), for a built-in command. */
  hint?: string;
  builtin: boolean;
}

/** The commands the composer itself runs; a skill of the same name is not listed. */
export const BUILTINS: SlashEntry[] = [
  { name: "compact", description: "Summarise the chat so far to free up context", hint: "[focus]", builtin: true },
];

/** The most skills and the longest description the menu takes (the server's own bounds). */
export const MAX_SKILLS = 150;
export const MAX_DESCRIPTION = 200;

/** The route's answer held to its shape; anything else lists no skills. */
export function readSkills(v: unknown): SlashEntry[] {
  const list = typeof v === "object" && v !== null ? (v as { skills?: unknown }).skills : null;
  if (!Array.isArray(list)) return [];
  const out: SlashEntry[] = [];
  for (const s of list) {
    if (out.length === MAX_SKILLS) break;
    if (typeof s !== "object" || s === null) continue;
    const { name, description } = s as Record<string, unknown>;
    if (typeof name !== "string" || !/^[A-Za-z0-9][\w.-]{0,63}$/.test(name)) continue;
    if (BUILTINS.some((b) => b.name === name) || out.some((o) => o.name === name)) continue;
    const d = typeof description === "string" ? Array.from(description).slice(0, MAX_DESCRIPTION).join("") : "";
    out.push({ name, description: d, builtin: false });
  }
  return out;
}

/** What the person sees of a description: hidden characters as codes. */
export const shownDescription = (d: string) => showHidden(d);

/** The entries whose name starts with `query`, then those that hold it, each group in list order. */
export function rank(entries: SlashEntry[], query: string): SlashEntry[] {
  const q = query.toLowerCase();
  const at = (e: SlashEntry) => e.name.toLowerCase().indexOf(q);
  return [...entries.filter((e) => at(e) === 0), ...entries.filter((e) => at(e) > 0)];
}

/** `text` with its first word made `/name `, and where the caret goes (after the space). */
export function complete(text: string, name: string): { text: string; caret: number } {
  const rest = text.replace(/^\/[\w.-]*/, "");
  const space = /^[ \t]/.test(rest) ? "" : " ";
  return { text: `/${name}${space}${rest}`, caret: name.length + 2 };
}

/** How long a chat's list is kept (a turn may add a skill; the person may add one by hand). */
export const KEEP_MS = 30_000;
const kept = new Map<string, { at: number; skills: SlashEntry[] }>();

/** The skills chat `id` can ask for: kept for [KEEP_MS] unless a turn ran (`running`: not kept). */
export async function loadSkills(id: string, running: boolean, signal: AbortSignal, now = Date.now()): Promise<SlashEntry[]> {
  const k = kept.get(id);
  if (k && now - k.at < KEEP_MS) return k.skills;
  const skills = readSkills(await call<unknown>("GET", `/api/sessions/${encodeURIComponent(id)}/skills-menu`, undefined, signal));
  if (running) kept.delete(id);
  else kept.set(id, { at: now, skills });
  if (kept.size > 20) kept.delete(kept.keys().next().value!);
  return skills;
}

/** Forget what is kept (tests). */
export const forgetSkills = () => kept.clear();

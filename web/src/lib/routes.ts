// The hash routes: home, a chat, a project, a settings tab, the agents,
// routines, media and artifacts. Small and dependency-free, since the first
// load parses them. Search has no route: it is an overlay.
import type { SectionId, TabId } from "./settings";

export type ProjectTab = "chats" | "board" | "memory" | "settings";
export type MediaFilter = "images" | "videos" | "audio" | "other";

export type Route =
  | { kind: "home" }
  | { kind: "session"; id: string }
  /** A project id, or "new" for the create form (docs/projects.md). */
  | { kind: "project"; id: string; tab?: ProjectTab }
  /** Unchecked: Settings validates the section and tab against what the viewer sees. */
  | { kind: "settings"; section?: string; tab?: string }
  /** id null: the list; `new` (NEW_AGENT in lib/agents.ts): a new agent; else that agent's page. */
  | { kind: "agents"; id: string | null }
  /** id null: the list; `new`: the editor, empty; else that routine, or its runs. */
  | { kind: "routines"; id: string | null; tab?: "runs" }
  /** The grid, maybe filtered; an id opens that item's preview over it. */
  | { kind: "media"; filter: MediaFilter | null; id: string | null }
  /** id null: the list; else that artifact (a lineage) with its versions. */
  | { kind: "artifacts"; id: string | null };

const PROJECT = /^#\/p\/(?:(new)|([0-9a-f]{32})(?:\/(chats|board|memory|settings))?)$/;
const ROUTINES = /^#\/routines(?:\/(new|[0-9a-f]{32})(?:\/(runs))?)?$/;
const MEDIA = /^#\/media(?:\/(images|videos|audio|other))?(?:\/([0-9a-f]{32}))?$/;
const ARTIFACTS = /^#\/artifacts(?:\/([0-9a-f]{32}))?$/;

/** The settings paths from before the sections, still accepted from old
 * bookmarks and notifications. */
const ALIASES: Record<string, [SectionId, TabId]> = {
  memory: ["account", "memory"],
  preferences: ["account", "preferences"],
  provider: ["server", "providers"],
  nodes: ["server", "nodes"],
};

export function parseRoute(hash: string): Route {
  const m = hash.match(/^#\/s\/([0-9a-f-]{36})$/);
  if (m) return { kind: "session", id: m[1]! };
  if (hash === "#/agents") return { kind: "agents", id: null };
  const a = hash.match(/^#\/agents\/([a-z0-9-]{1,32})$/);
  if (a) return { kind: "agents", id: a[1]! };
  // Agents moved out of Server settings into the sidebar.
  if (hash === "#/settings/server/agents") return { kind: "agents", id: null };
  const p = hash.match(PROJECT);
  if (p) return p[1] ? { kind: "project", id: "new" } : { kind: "project", id: p[2]!, ...(p[3] ? { tab: p[3] as ProjectTab } : {}) };
  const r = hash.match(ROUTINES);
  // A routine not saved yet has no runs.
  if (r && !(r[1] === "new" && r[2])) return { kind: "routines", id: r[1] ?? null, ...(r[2] ? { tab: "runs" as const } : {}) };
  const md = hash.match(MEDIA);
  if (md) return { kind: "media", filter: (md[1] as MediaFilter | undefined) ?? null, id: md[2] ?? null };
  const art = hash.match(ARTIFACTS);
  if (art) return { kind: "artifacts", id: art[1] ?? null };
  const s = hash.match(/^#\/settings(?:\/([a-z-]+))?(?:\/([a-z-]+))?$/);
  if (!s) return { kind: "home" };
  const [, section, tab] = s;
  const alias = section && !tab ? ALIASES[section] : undefined;
  if (alias) return { kind: "settings", section: alias[0], tab: alias[1] };
  return { kind: "settings", ...(section ? { section } : {}), ...(tab ? { tab } : {}) };
}

export function routeHash(r: Route): string {
  if (r.kind === "session") return `#/s/${r.id}`;
  if (r.kind === "agents") return r.id === null ? "#/agents" : `#/agents/${r.id}`;
  if (r.kind === "project") return `#/p/${r.id}${r.tab ? `/${r.tab}` : ""}`;
  if (r.kind === "routines") return `#/routines${r.id ? `/${r.id}${r.tab && r.id !== "new" ? `/${r.tab}` : ""}` : ""}`;
  if (r.kind === "media") return `#/media${r.filter ? `/${r.filter}` : ""}${r.id ? `/${r.id}` : ""}`;
  if (r.kind === "artifacts") return `#/artifacts${r.id ? `/${r.id}` : ""}`;
  if (r.kind === "settings") return `#/settings${r.section ? `/${r.section}${r.tab ? `/${r.tab}` : ""}` : ""}`;
  return "#/";
}

export function settingsHash(section: SectionId, tab?: TabId): string {
  return routeHash({ kind: "settings", section, tab });
}

export const projectHash = (id: string, tab?: ProjectTab): string => routeHash({ kind: "project", id, tab });
export const routineHash = (id: string | null, tab?: "runs"): string => routeHash({ kind: "routines", id, tab });
export const mediaHash = (filter: MediaFilter | null, id: string | null = null): string =>
  routeHash({ kind: "media", filter, id });
export const artifactHash = (id: string | null): string => routeHash({ kind: "artifacts", id });

/** Links other screens use. */
export const MEMORY_HASH = settingsHash("account", "memory");

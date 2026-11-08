// The hash routes: home, a chat, a project, a settings tab, the agents.
// Small and dependency-free, since the first load parses them.
import type { SectionId, TabId } from "./settings";

export type ProjectTab = "chats" | "board" | "memory" | "settings";

export type Route =
  | { kind: "home" }
  | { kind: "session"; id: string }
  /** A project id, or "new" for the create form (docs/projects.md). */
  | { kind: "project"; id: string; tab?: ProjectTab }
  /** Unchecked: Settings validates the section and tab against what the viewer sees. */
  | { kind: "settings"; section?: string; tab?: string }
  /** id null: the list; `new` (NEW_AGENT in lib/agents.ts): a new agent; else that agent's page. */
  | { kind: "agents"; id: string | null };

const PROJECT = /^#\/p\/(?:(new)|([0-9a-f]{32})(?:\/(chats|board|memory|settings))?)$/;

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
  if (r.kind === "settings") return `#/settings${r.section ? `/${r.section}${r.tab ? `/${r.tab}` : ""}` : ""}`;
  return "#/";
}

export function settingsHash(section: SectionId, tab?: TabId): string {
  return routeHash({ kind: "settings", section, tab });
}

export const projectHash = (id: string, tab?: ProjectTab): string => routeHash({ kind: "project", id, tab });

/** Links other screens use. */
export const MEMORY_HASH = settingsHash("account", "memory");
export const NODES_HASH = settingsHash("server", "nodes");

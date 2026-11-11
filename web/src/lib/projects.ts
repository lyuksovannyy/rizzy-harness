// Projects in the browser (docs/projects.md): types and API calls, kept out
// of api.ts and types.ts so the page's code stays in its lazy chunk.
import { call } from "../api";
import type { MemoryFact } from "../types";

/** `?workspace=` for list calls: the Shell's workspace, never the server's idea of "active". */
const inWorkspace = (path: string, workspace: string | null | undefined) =>
  workspace ? `${path}?workspace=${encodeURIComponent(workspace)}` : path;

export interface ProjectSettings {
  /** The project's chats also recall the workspace's shared facts (once those exist). */
  workspace_memory: boolean;
  /** Members may confirm their own fact proposals (otherwise a manager must). */
  members_activate_facts: boolean;
  node_id: string | null;
  cwd: string | null;
}

export interface ProjectSummary {
  project_id: string;
  workspace_id: string;
  name: string;
  description: string;
  icon: string;
  accent: string;
  /** The caller's own chats in it. */
  chats: number;
  updated_ms: number;
}

export interface ProjectDetail extends ProjectSummary {
  instructions: string;
  settings: ProjectSettings;
  can_manage: boolean;
  created_by: string | null;
  created_ms: number;
}

export interface ProjectList {
  projects: ProjectSummary[];
  workspace_id: string | null;
  can_create: boolean;
  icons: string[];
  accents: string[];
}

export interface BoardCard {
  card_id: string;
  column_id: string;
  title: string;
  notes: string;
  position: number;
  /** Someone else's chat shows as "private". */
  chat: { session_id: string; title: string } | "private" | null;
  created_by: string | null;
  updated_ms: number;
}

export interface BoardColumn {
  column_id: string;
  name: string;
  done: boolean;
  cards: BoardCard[];
}

export interface Board {
  columns: BoardColumn[];
}

export interface BoardTemplate {
  template_id: string;
  name: string;
  builtin: boolean;
  columns: { name: string; done: boolean }[];
}

/** Limits the server enforces (crates/master/src/projects.rs). */
export const LIMITS = { name: 64, description: 280, instructions: 4000, title: 200, notes: 4000, column: 40 } as const;

const base = (id: string) => `/api/projects/${id}`;

export const projectsApi = {
  list: (workspace?: string | null) => call<ProjectList>("GET", inWorkspace("/api/projects", workspace)),
  create: (b: {
    name: string;
    description?: string;
    icon?: string;
    accent?: string;
    template_id?: string;
    instructions?: string;
    workspace_id?: string;
  }) => call<ProjectDetail>("POST", "/api/projects", b),
  get: (id: string) => call<ProjectDetail>("GET", base(id)),
  update: (
    id: string,
    b: Partial<Pick<ProjectDetail, "name" | "description" | "icon" | "accent" | "instructions" | "settings">>,
  ) => call<ProjectDetail>("PATCH", base(id), b),
  remove: (id: string) => call<{ ok: boolean }>("DELETE", base(id)),
  /** Put a chat into a project, or take it out (null). */
  setChatProject: (sessionId: string, projectId: string | null) =>
    call<{ ok: boolean }>("PUT", `/api/sessions/${sessionId}/project`, { project_id: projectId }),
  board: (id: string) => call<Board>("GET", `${base(id)}/board`),
  addColumn: (id: string, name: string) => call<Board>("POST", `${base(id)}/board/columns`, { name }),
  updateColumn: (id: string, col: string, b: { name?: string; done?: boolean; position?: number }) =>
    call<Board>("PATCH", `${base(id)}/board/columns/${col}`, b),
  deleteColumn: (id: string, col: string) => call<Board>("DELETE", `${base(id)}/board/columns/${col}`),
  addCard: (id: string, b: { column_id: string; title: string; notes?: string }) =>
    call<Board>("POST", `${base(id)}/board/cards`, b),
  updateCard: (
    id: string,
    card: string,
    b: { title?: string; notes?: string; column_id?: string; position?: number; session_id?: string | null },
  ) => call<Board>("PATCH", `${base(id)}/board/cards/${card}`, b),
  deleteCard: (id: string, card: string) => call<Board>("DELETE", `${base(id)}/board/cards/${card}`),
  applyTemplate: (id: string, templateId: string) =>
    call<Board>("POST", `${base(id)}/board/template`, { template_id: templateId }),
  templates: (workspace?: string | null) =>
    call<{ templates: BoardTemplate[] }>("GET", inWorkspace("/api/board-templates", workspace)),
  saveTemplate: (name: string, projectId: string) =>
    call<BoardTemplate>("POST", "/api/board-templates", { name, project_id: projectId }),
  deleteTemplate: (id: string) => call<{ ok: boolean }>("DELETE", `/api/board-templates/${id}`),
  memories: (id: string) => call<{ memories: MemoryFact[] }>("GET", `${base(id)}/memory`),
  addMemory: (id: string, text: string) => call<MemoryFact>("POST", `${base(id)}/memory`, { text }),
  updateMemory: (id: string, fact: string, b: { text?: string; status?: "active" }) =>
    call<MemoryFact>("PATCH", `${base(id)}/memory/${fact}`, b),
  deleteMemory: (id: string, fact: string) => call<{ ok: boolean }>("DELETE", `${base(id)}/memory/${fact}`),
};

/** Where a card dropped before `before` (a card id; null = the end) lands in
 * `column`'s cards, the dragged card itself left out: the index to send. */
export function dropIndex(column: BoardColumn, dragged: string, before: string | null): number {
  // Dropped on itself: it stays where it is.
  if (before === dragged) return Math.max(0, column.cards.findIndex((c) => c.card_id === dragged));
  const rest = column.cards.filter((c) => c.card_id !== dragged);
  const i = before === null ? -1 : rest.findIndex((c) => c.card_id === before);
  return i < 0 ? rest.length : i;
}

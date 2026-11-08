// The sidebar's Projects section and the mark a project's chats carry in
// Recents (docs/projects.md). Self-contained, so the sidebar can move it.
import { Plus } from "lucide-react";
import { t } from "../lib/i18n";
import { ProjectIcon } from "../lib/projectIcons";
import type { ProjectSummary } from "../lib/projects";

/** The workspace's projects with a + for a new one; nothing when there are
 * none and the viewer may not create one. */
export function ProjectNav(props: {
  projects: ProjectSummary[];
  canCreate: boolean;
  selected: string | null;
  /** A project's id, or "new" for the create form. */
  onOpen: (id: string) => void;
}) {
  if (props.projects.length === 0 && !props.canCreate) return null;
  return (
    <>
      <div className="section-label section-row">
        <span className="grow">{t("nav.projects")}</span>
        {props.canCreate && (
          <button className="icon small" aria-label={t("nav.newProject")} title={t("nav.newProject")} onClick={() => props.onOpen("new")}>
            <Plus size={14} />
          </button>
        )}
      </div>
      <nav className="project-list">
        {props.projects.map((p) => (
          <button
            key={p.project_id}
            className={`session-item ${p.project_id === props.selected ? "active" : ""}`}
            onClick={() => props.onOpen(p.project_id)}
            title={p.description || p.name}
          >
            <ProjectIcon icon={p.icon} accent={p.accent} />
            <span className="ellipsis grow">{p.name}</span>
          </button>
        ))}
      </nav>
    </>
  );
}

/** A chat's project icon in a chat list; nothing outside a listed project. */
export function ChatProjectMark({ id, projects }: { id: string | null | undefined; projects: ProjectSummary[] }) {
  const p = id ? projects.find((x) => x.project_id === id) : undefined;
  if (!p) return null;
  return (
    <span className="chat-project" title={p.name}>
      <ProjectIcon icon={p.icon} accent={p.accent} size={13} />
    </span>
  );
}

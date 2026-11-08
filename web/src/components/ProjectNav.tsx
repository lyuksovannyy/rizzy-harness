// The sidebar's project rows and the mark a project's chats carry in Recent
// (docs/projects.md, docs/sidebar-search.md).
import { useState } from "react";
import { t } from "../lib/i18n";
import { ProjectIcon } from "../lib/projectIcons";
import type { ProjectSummary } from "../lib/projects";
import type { Route } from "../lib/routes";
import { NavLink } from "./NavLink";

/** How many projects show before "Show N more". */
const FIRST = 5;

/** The workspace's projects, most recently changed first: the first five,
 * then the rest on request. */
export function ProjectRows(props: {
  projects: ProjectSummary[];
  /** The open project's id, if one is open. */
  selected: string | null;
  onNavigate: (r: Route) => void;
}) {
  const [all, setAll] = useState(false);
  if (props.projects.length === 0) return <div className="muted pad small">{t("nav.noProjects")}</div>;
  const sorted = [...props.projects].sort((a, b) => b.updated_ms - a.updated_ms);
  const shown = all ? sorted : sorted.slice(0, FIRST);
  const more = sorted.length - FIRST;
  return (
    <>
      {shown.map((p) => (
        <NavLink
          key={p.project_id}
          to={{ kind: "project", id: p.project_id }}
          current={p.project_id === props.selected}
          onNavigate={props.onNavigate}
          className="session-item"
          title={p.description || p.name}
        >
          <ProjectIcon icon={p.icon} accent={p.accent} />
          <span className="ellipsis grow">{p.name}</span>
        </NavLink>
      ))}
      {more > 0 && (
        <button type="button" className="link small show-more" onClick={() => setAll((v) => !v)}>
          {all ? t("nav.showFewer") : t("nav.showMore", { n: String(more) })}
        </button>
      )}
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

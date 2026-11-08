// The chat header's project picker: shows the chat's project and moves the
// chat into another one or out (docs/projects.md). A native select keeps it
// small and keyboard-friendly; the icon sits on top of it. Also what a
// project-scoped `remember` shows in the chat.
import { BookmarkPlus, FolderPlus } from "lucide-react";
import { useState } from "react";
import { projectsApi, type ProjectSummary } from "../lib/projects";
import { ProjectIcon } from "../lib/projectIcons";
import { projectHash } from "../lib/routes";
import { showHidden } from "../md/hidden";
import { rt } from "../md/strings";
import type { SessionRow } from "../types";
import { MemoryNotice } from "./MemoryNotice";

/** A `remember` with scope "project" in the chat of `project`: it waits on
 * the project's Memory tab. Outside a project the server filed it as a
 * personal proposal. */
export function ProjectMemoryNotice({ text, project }: { text: string; project: string | null }) {
  if (!project) return <MemoryNotice text={text} />;
  return (
    <div className="tool tool-memory" role="note">
      <BookmarkPlus size={14} aria-hidden="true" className="memory-icon" />
      <div className="memory-body">
        <div>
          Suggested for the project: <span className="memory-fact">“{showHidden(text.trim())}”</span>
        </div>
        <div className="muted small">
          {rt("memory.pending")}{" "}
          <a className="memory-review" href={projectHash(project, "memory")}>
            Review in the project
          </a>
        </div>
      </div>
    </div>
  );
}

/** Temporary chats and sub-tasks cannot be in a project. */
export function canJoinProject(row: SessionRow | null): boolean {
  return !!row && !row.incognito && !row.parent_id;
}

export function ProjectChip({ row, projects }: { row: SessionRow; projects: ProjectSummary[] }) {
  const [error, setError] = useState<string | null>(null);
  const current = projects.find((p) => p.project_id === row.project_id) ?? null;
  if (!current && projects.length === 0) return null;
  const move = (id: string) => {
    setError(null);
    projectsApi.setChatProject(row.session_id, id || null).catch((e: Error) => setError(e.message));
  };
  return (
    <label className={`project-chip ${current ? "on" : ""}`} title={error ?? (current ? `In ${current.name}` : "Add to a project")}>
      {current ? <ProjectIcon icon={current.icon} accent={current.accent} size={14} /> : <FolderPlus size={14} aria-hidden />}
      <span className="ellipsis">{current ? current.name : "Project"}</span>
      <select aria-label="Project" value={current?.project_id ?? ""} onChange={(e) => move(e.target.value)}>
        <option value="">No project</option>
        {projects.map((p) => (
          <option key={p.project_id} value={p.project_id}>
            {p.name}
          </option>
        ))}
      </select>
    </label>
  );
}

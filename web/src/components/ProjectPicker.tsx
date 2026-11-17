// The start screen's Project button and its popup (ADOPTION_PLAN X.26):
// where a new agentic chat works. A project of the workspace (its node and folder), a
// new project (the node makes its folder, a git repository, from the name;
// nothing here names a path), or an existing folder on the node (today's
// free path, which the node bounds by its workspace_roots). "From a
// repository" comes later (X.27). Its own lazy chunk, loaded with the
// agentic row (the composer is first-load code).
import { ChevronDown, GitBranch, Search } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { LIMITS, type ProjectList, type ProjectSummary, projectsApi } from "../lib/projects";
import { ProjectIcon } from "../lib/projectIcons";
import { projectSlug } from "../lib/projectSlug";
import { shownName } from "../md/hiddenChars";
import type { NodeInfo } from "../types";

/** What the person chose: the chat's folder and its project, with the
 * project's node; `nodeId` empty for a folder (the node chosen stays). */
export interface ProjectChoice {
  nodeId: string;
  cwd: string;
  project: { id: string; name: string; icon: string; accent: string } | null;
}

/** The folders new chats were started in, most recent first (written by
 * lib/startChat.ts; kept here so the popup needs no chunk of its own). */
function recentFolders(): string[] {
  try {
    return JSON.parse(localStorage.getItem("harness.folders") ?? "[]") as string[];
  } catch {
    return [];
  }
}

const nodeName = (n: NodeInfo) => (n.embedded ? `local (${shownName(n.hostname)})` : shownName(n.hostname));

interface Props {
  /** The node chosen in the row. */
  nodeId: string;
  workers: NodeInfo[];
  workspace?: string | null;
  /** The folder now, and the project now. */
  cwd: string;
  project?: ProjectChoice["project"];
  /** A project's own page: its chats are in it already; only a folder is chosen. */
  fixed?: boolean;
  onPick: (c: ProjectChoice) => void;
}

/** The last part of a folder's path, for the button. */
export const lastPart = (cwd: string) => cwd.trim().replace(/\/+$/, "").split("/").pop() || "/";

/** The button: the project (its icon and name), else the folder's last
 * part, else "No project"; the full folder in its tooltip. */
export default function ProjectButton(props: Props) {
  const [open, setOpen] = useState(false);
  const p = props.project;
  // Nothing chosen yet: the folder the latest chat was started in, as the
  // folder field offered before (once, when the row first shows).
  useEffect(() => {
    const last = recentFolders()[0];
    if (!props.cwd && !p && last) props.onPick({ nodeId: "", cwd: last, project: null });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  return (
    <label>
      Project
      <button
        type="button"
        className="pill project-pick"
        aria-haspopup="dialog"
        title={props.cwd || undefined}
        onClick={() => setOpen(true)}
      >
        {p && <ProjectIcon icon={p.icon} accent={p.accent} size={14} />}
        {p ? p.name : props.cwd.trim() ? lastPart(props.cwd) : "No project"}
        <ChevronDown size={14} aria-hidden />
      </button>
      {open && <ProjectPicker {...props} onClose={() => setOpen(false)} />}
    </label>
  );
}

export function ProjectPicker(props: Props & { onClose: () => void }) {
  const projectId = props.project?.id ?? null;
  const dialog = useRef<HTMLDialogElement>(null);
  const [list, setList] = useState<ProjectList | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [q, setQ] = useState("");
  const [name, setName] = useState("");
  const [folder, setFolder] = useState(projectId ? "" : props.cwd);
  const [busy, setBusy] = useState<string | null>(null);
  const node = props.workers.find((w) => w.node_id === props.nodeId);
  useEffect(() => {
    const d = dialog.current;
    if (d && typeof d.showModal === "function" && !d.open) d.showModal();
  }, []);
  useEffect(() => {
    if (props.fixed) return;
    let live = true;
    projectsApi.list(props.workspace).then(
      (l) => live && setList(l),
      (e: unknown) => live && setErr(e instanceof Error ? e.message : String(e)),
    );
    return () => {
      live = false;
    };
  }, [props.fixed, props.workspace]);
  const close = () => {
    dialog.current?.close?.();
    props.onClose();
  };
  const pick = (c: ProjectChoice) => {
    dialog.current?.close?.();
    props.onPick(c);
    props.onClose();
  };
  const usable = (p: ProjectSummary) => (p.node_id ? props.workers.find((w) => w.node_id === p.node_id) : undefined);
  const choose = (p: ProjectSummary) =>
    pick({ nodeId: p.node_id!, cwd: p.cwd!, project: { id: p.project_id, name: p.name, icon: p.icon, accent: p.accent } });
  const run = async (what: string, f: () => Promise<void>) => {
    setBusy(what);
    setErr(null);
    try {
      await f();
    } catch (e) {
      setErr(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(null);
    }
  };
  const shown = useMemo(() => {
    const words = q.trim().toLowerCase();
    const all = list?.projects ?? [];
    return words ? all.filter((p) => p.name.toLowerCase().includes(words) || (p.cwd ?? "").toLowerCase().includes(words)) : all;
  }, [list, q]);
  const mayMake = !!list?.can_create && !!node?.projects;
  return (
    <dialog
      ref={dialog}
      className="project-picker"
      aria-labelledby="project-picker-title"
      onCancel={(e) => {
        e.preventDefault();
        close();
      }}
      onClick={(e) => e.target === dialog.current && close()}
    >
      <div className="stack">
        <h2 id="project-picker-title">{props.fixed ? "Folder for this chat" : "Where this chat works"}</h2>
        {!props.fixed && (
          <section aria-labelledby="pp-projects">
            <h3 id="pp-projects">Projects</h3>
            <label className="pp-search">
              <Search size={14} aria-hidden />
              <input
                type="search"
                aria-label="Search projects"
                placeholder="Search projects"
                value={q}
                autoFocus
                onChange={(e) => setQ(e.target.value)}
              />
            </label>
            {!list && !err && <p className="muted">Loading…</p>}
            {list && !shown.length && <p className="muted">{q.trim() ? "No project matches." : "No projects yet."}</p>}
            <ul className="pp-list">
              {shown.map((p) => {
                const n = usable(p);
                const here = p.node_id === props.nodeId;
                return (
                  <li key={p.project_id}>
                    {p.cwd && p.node_id ? (
                      <button
                        type="button"
                        className={`pp-row${p.project_id === projectId ? " on" : ""}`}
                        disabled={!n || !!busy}
                        aria-current={p.project_id === projectId || undefined}
                        onClick={() => choose(p)}
                      >
                        <ProjectIcon icon={p.icon} accent={p.accent} />
                        <span className="pp-name">{p.name}</span>
                        <span className="pp-where mono" title={p.cwd}>
                          {p.cwd}
                          {!here && (n ? ` · on ${nodeName(n)}` : ` · its node ${p.node_id} is not connected`)}
                        </span>
                      </button>
                    ) : (
                      <div className="pp-row">
                        <ProjectIcon icon={p.icon} accent={p.accent} />
                        <span className="pp-name">{p.name}</span>
                        <span className="pp-where muted">no folder yet</span>
                        {mayMake && node && (
                          <button
                            type="button"
                            className="pill"
                            disabled={!!busy}
                            onClick={() =>
                              run(p.project_id, async () => {
                                const d = await projectsApi.giveFolder(p.project_id, node.node_id);
                                choose({ ...p, node_id: d.settings.node_id, cwd: d.settings.cwd });
                              })
                            }
                          >
                            {busy === p.project_id ? "Creating…" : `Create its folder on ${nodeName(node)}`}
                          </button>
                        )}
                      </div>
                    )}
                  </li>
                );
              })}
            </ul>
          </section>
        )}
        {!props.fixed && (
          <section aria-labelledby="pp-new">
            <h3 id="pp-new">New project</h3>
            {list && !list.can_create ? (
              <p className="muted">Your role in this workspace cannot create projects.</p>
            ) : node && !node.projects ? (
              <p className="muted">
                {nodeName(node)} makes no project folders (its node.toml, [projects] dir). Choose an existing folder below.
              </p>
            ) : (
              <form
                className="stack"
                onSubmit={(e) => {
                  e.preventDefault();
                  if (!node || !name.trim()) return;
                  void run("new", async () => {
                    const d = await projectsApi.create({
                      name: name.trim(),
                      folder_on: node.node_id,
                      ...(props.workspace ? { workspace_id: props.workspace } : {}),
                    });
                    pick({
                      nodeId: d.settings.node_id!,
                      cwd: d.settings.cwd!,
                      project: { id: d.project_id, name: d.name, icon: d.icon, accent: d.accent },
                    });
                  });
                }}
              >
                <input
                  aria-label="Project name"
                  placeholder="Project name"
                  maxLength={LIMITS.name}
                  value={name}
                  onChange={(e) => setName(e.target.value)}
                />
                <p className="pp-preview muted">
                  A new git repository, <code>{projectSlug(name)}-····</code>, in {node ? nodeName(node) : "the node"}'s projects folder.
                </p>
                <div className="row end">
                  <button type="submit" className="primary" disabled={!mayMake || !name.trim() || !!busy}>
                    {busy === "new" ? "Creating…" : "Create"}
                  </button>
                </div>
              </form>
            )}
          </section>
        )}
        <section aria-labelledby="pp-folder">
          <h3 id="pp-folder">Existing folder</h3>
          <form
            className="row"
            onSubmit={(e) => {
              e.preventDefault();
              if (folder.trim()) pick({ nodeId: "", cwd: folder.trim(), project: null });
            }}
          >
            <input
              className="mono grow"
              aria-label="Folder on that node"
              list="recent-folders"
              placeholder="/home/me/project"
              value={folder}
              autoFocus={props.fixed}
              onChange={(e) => setFolder(e.target.value)}
            />
            <datalist id="recent-folders">
              {recentFolders().map((f) => (
                <option key={f} value={f} />
              ))}
            </datalist>
            <button type="submit" disabled={!folder.trim()}>
              Use folder
            </button>
          </form>
        </section>
        {!props.fixed && (
          <section aria-labelledby="pp-repo">
            <h3 id="pp-repo">From a repository</h3>
            <button type="button" className="pp-row" disabled aria-describedby="pp-later">
              <GitBranch size={16} aria-hidden />
              <span className="pp-name">Clone from GitHub, GitLab, Gitea or Forgejo</span>
              <span id="pp-later" className="badge">
                Later
              </span>
            </button>
          </section>
        )}
        {err && (
          <p className="error-text" role="alert">
            {err}
          </p>
        )}
        <div className="row end">
          <button type="button" onClick={close}>
            Cancel
          </button>
        </div>
      </div>
    </dialog>
  );
}

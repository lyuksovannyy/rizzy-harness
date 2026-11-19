// The start screen's Project button and its popup (ADOPTION_PLAN X.26):
// where a new agentic chat works. A project of the workspace (its node and folder), a
// new project (the node makes its folder, a git repository, from the name;
// nothing here names a path), or an existing folder on the node (today's
// free path, which the node bounds by its workspace_roots), or a clone of a
// forge repository (X.27: its own lazy chunk, RepoPicker). A project whose
// folder is being cloned shows as cloning and cannot be picked; a failed
// one shows its reason, with Dismiss, to its starter. Its own lazy chunk,
// loaded with the agentic row (the composer is first-load code).
import { ChevronDown, Search } from "lucide-react";
import { lazy, Suspense, useEffect, useMemo, useRef, useState, useSyncExternalStore } from "react";
import { LIMITS, type PendingProject, type ProjectList, type ProjectSummary, pendingProjects, projectsApi } from "../lib/projects";
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

const RepoPicker = lazy(() => import("./RepoPicker"));

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
  }, []);
  // The dialog stays outside the label: a click in it must not reach the
  // button the label names, and the button's name is the label's text.
  return (
    <>
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
      </label>
      {open && <ProjectPicker {...props} onClose={() => setOpen(false)} />}
    </>
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
  const [started, setStarted] = useState<PendingProject | null>(null);
  const startedLine = useRef<HTMLParagraphElement>(null);
  const node = props.workers.find((w) => w.node_id === props.nodeId);
  // Bumped by the Shell on every `projects_changed`: a clone that ends
  // shows here too.
  const changed = useSyncExternalStore(pendingProjects.sub, pendingProjects.get);
  const [reload, setReload] = useState(0);
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
  }, [props.fixed, props.workspace, changed, reload]);
  useEffect(() => {
    if (started) startedLine.current?.focus();
  }, [started]);
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
            {list && !shown.length && !list.pending?.length && <p className="muted">{q.trim() ? "No project matches." : "No projects yet."}</p>}
            <ul className="pp-list">
              {list?.pending?.map((c) => (
                <li key={c.project_id}>
                  <div className="pp-row">
                    <span className="pp-name">{shownName(c.name)}</span>
                    <span className="pp-where muted" title={c.reason ? shownName(c.reason) : undefined}>
                      {c.state === "failed"
                        ? `Clone failed: ${shownName(c.reason ?? "")}`
                        : `Cloning ${shownName(`${c.source.host}/${c.source.full_name}`)}…`}
                    </span>
                    {c.state === "failed" && c.mine && (
                      <button
                        type="button"
                        className="pill"
                        disabled={!!busy}
                        onClick={() =>
                          run(c.project_id, async () => {
                            await projectsApi.dismissClone(c.project_id);
                            setReload((n) => n + 1);
                          })
                        }
                      >
                        Dismiss
                      </button>
                    )}
                  </div>
                </li>
              ))}
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
            {started ? (
              <p ref={startedLine} tabIndex={-1} role="status">
                Cloning {shownName(`${started.source.host}/${started.source.full_name}`)}… You can close this: the project
                appears in your list when its node is done.
              </p>
            ) : list && !list.can_create ? (
              <p className="muted">Your role in this workspace cannot create projects.</p>
            ) : (
              <Suspense fallback={<p className="muted">Loading…</p>}>
                <RepoPicker
                  workspace={props.workspace}
                  node={node}
                  nodeLabel={node ? nodeName(node) : "the node"}
                  canCreate={!!list?.can_create}
                  onStarted={(p) => {
                    setStarted(p);
                    setReload((n) => n + 1);
                  }}
                />
              </Suspense>
            )}
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

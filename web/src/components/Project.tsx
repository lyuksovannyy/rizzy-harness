// A project's page (docs/projects.md): a bar that starts chats inside it,
// then Chats, Board, Memory, Artifacts (what its chats published, while the
// server keeps them) and Settings. Also the create form (`#/p/new`).
// Loaded on demand; the strings live here, not in the first-load catalog.
import { ArrowLeft, ArrowRight, MessageSquare, Trash2, X } from "lucide-react";
import { type CSSProperties, type DragEvent, useCallback, useEffect, useRef, useState } from "react";
import { HttpError } from "../api";
import { type Artifact, artifactsApi, latestUrl } from "../lib/artifacts";
import { useFeatures } from "../lib/features";
import {
  type Board,
  type BoardCard,
  type BoardColumn,
  type BoardTemplate,
  dropIndex,
  LIMITS,
  type ProjectDetail,
  projectsApi,
} from "../lib/projects";
import { artifactHash, type ProjectTab } from "../lib/routes";
import { accentColor, PROJECT_ICONS, ProjectIcon } from "../lib/projectIcons";
import { radioKeys, radioTab } from "../lib/radios";
import type { Agent, MemoryFact, ModelInfo, NodeInfo, Prefs, SessionRow } from "../types";
import { Composer, type Submission } from "./Composer";
import { ACCENTS } from "./forms";

// Artifacts has no route of its own (lib/routes.ts): the tab is this page's.
type Tab = ProjectTab | "artifacts";
const TABS: [Tab, string][] = [
  ["chats", "Chats"],
  ["board", "Board"],
  ["memory", "Memory"],
  ["artifacts", "Artifacts"],
  ["settings", "Settings"],
];

const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

export default function Project(props: {
  /** A project id, or "new". */
  id: string;
  /** The tab a link opened (`#/p/<id>/memory`). */
  tab?: ProjectTab;
  /** Bumped on every `projects_changed`. */
  version: number;
  memoryVersion: number;
  /** The caller's listed chats. */
  chats: SessionRow[];
  agents: Agent[];
  defaultAgent: string;
  models: ModelInfo[];
  workers: NodeInfo[];
  nodes: NodeInfo[];
  prefs: Prefs | null;
  agenticBlocked: string | null;
  voice?: boolean;
  onSubmit: (s: Submission, projectId?: string) => Promise<void>;
  onOpenChat: (sessionId: string) => void;
  onOpenProject: (projectId: string) => void;
  onDeleted: () => void;
  /** The workspace this page lists in (null: the API token). */
  workspaceId: string | null;
  /** A link opened a project of another of the user's workspaces: switch there. */
  onForeign: (workspaceId: string) => void;
}) {
  const [project, setProject] = useState<ProjectDetail | null>(null);
  const [gone, setGone] = useState(false);
  const [tab, setTab] = useState<Tab>(props.tab ?? "chats");
  useEffect(() => {
    if (props.tab) setTab(props.tab);
  }, [props.tab]);
  const [agentId, setAgentId] = useState(props.defaultAgent);
  // The default follows the preferences, which may arrive after this page.
  useEffect(() => setAgentId(props.defaultAgent), [props.defaultAgent]);
  const { id, version } = props;
  // What the server offers here: the Artifacts tab, and images on the bar.
  const features = useFeatures(props.workspaceId, id !== "new");
  const artifactsShown = !!features?.artifacts.show;

  useEffect(() => {
    if (id === "new") return;
    projectsApi
      .get(id)
      .then(setProject)
      .catch((e) => e instanceof HttpError && e.status === 404 && setGone(true));
  }, [id, version]);
  // Like a chat, a project shows in its own workspace: once per target (the
  // switch remounts the Shell there), whatever callback the parent passes.
  const foreign = project && props.workspaceId && project.workspace_id !== props.workspaceId ? project.workspace_id : null;
  const onForeign = useRef(props.onForeign);
  useEffect(() => {
    onForeign.current = props.onForeign;
  });
  useEffect(() => {
    if (foreign) onForeign.current(foreign);
  }, [foreign]);

  if (id === "new") return <NewProject workspaceId={props.workspaceId} onCreated={props.onOpenProject} />;
  if (gone) return <section className="settings"><div className="column wide muted">This project no longer exists.</div></section>;
  if (!project) return <section className="settings"><div className="column wide muted">Loading…</div></section>;

  const style = { "--project": accentColor(project.accent) } as CSSProperties;
  const own = props.chats.filter((c) => c.project_id === project.project_id);
  return (
    <section className="settings project-page" style={style}>
      <div className="column wide">
        <header className="project-head">
          <span className="project-badge">
            <ProjectIcon icon={project.icon} accent={project.accent} size={22} />
          </span>
          <div className="grow">
            <h1>{project.name}</h1>
            {project.description && <p className="muted">{project.description}</p>}
          </div>
        </header>
        <Composer
          where="home"
          agents={props.agents}
          models={props.models}
          workers={props.workers}
          agentId={agentId}
          onAgentChange={setAgentId}
          defaultModel={props.prefs?.default_model}
          agenticBlocked={props.agenticBlocked}
          onSubmit={(s) => props.onSubmit(s, project.project_id)}
          voice={props.voice}
          project={{ nodeId: project.settings.node_id, cwd: project.settings.cwd }}
          images={features?.media.vision_images ? { workspace: props.workspaceId } : null}
        />
        <div className="tabs" role="tablist">
          {TABS.filter(([x]) => x !== "artifacts" || artifactsShown).map(([x, label]) => (
            <button key={x} role="tab" aria-selected={tab === x} className={tab === x ? "on" : ""} onClick={() => setTab(x)}>
              {label}
            </button>
          ))}
        </div>
        {tab === "chats" && <Chats project={project} own={own} all={props.chats} onOpen={props.onOpenChat} />}
        {tab === "board" && <BoardView project={project} chats={own} version={version} onOpen={props.onOpenChat} />}
        {tab === "memory" && <ProjectMemory project={project} version={props.memoryVersion} onOpen={props.onOpenChat} />}
        {tab === "artifacts" && <ProjectArtifacts project={project} workspaceId={props.workspaceId} />}
        {tab === "settings" && (
          <ProjectSettings project={project} nodes={props.nodes} onSaved={setProject} onDeleted={props.onDeleted} />
        )}
      </div>
    </section>
  );
}

/** Icon grid and accent swatches, shared by the create form and Settings. */
function LookFields(props: { icon: string; accent: string; onIcon: (i: string) => void; onAccent: (a: string) => void; disabled?: boolean }) {
  return (
    <>
      <div className="field">
        Icon
        <div className="icon-grid" role="radiogroup" aria-label="icon" onKeyDown={radioKeys}>
          {Object.keys(PROJECT_ICONS).map((name) => (
            <button
              key={name}
              type="button"
              role="radio"
              aria-checked={props.icon === name}
              aria-label={name}
              tabIndex={radioTab(props.icon === name)}
              className={props.icon === name ? "on" : ""}
              disabled={props.disabled}
              onClick={() => props.onIcon(name)}
            >
              <ProjectIcon icon={name} accent={props.icon === name ? props.accent : "gray"} size={18} />
            </button>
          ))}
        </div>
      </div>
      <div className="field">
        Accent
        <div className="row swatches" role="radiogroup" aria-label="accent" onKeyDown={radioKeys}>
          {ACCENTS.map((a) => (
            <button
              key={a}
              type="button"
              role="radio"
              aria-checked={props.accent === a}
              tabIndex={radioTab(props.accent === a)}
              aria-label={a}
              className={`swatch sw-${a} ${props.accent === a ? "on" : ""}`}
              disabled={props.disabled}
              onClick={() => props.onAccent(a)}
            />
          ))}
        </div>
      </div>
    </>
  );
}

function NewProject({ workspaceId, onCreated }: { workspaceId: string | null; onCreated: (id: string) => void }) {
  const [name, setName] = useState("");
  const [description, setDescription] = useState("");
  const [icon, setIcon] = useState("folder");
  const [accent, setAccent] = useState("blue");
  const [templates, setTemplates] = useState<BoardTemplate[]>([]);
  const [template, setTemplate] = useState("basic");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    projectsApi
      .templates(workspaceId)
      .then((r) => setTemplates(r.templates))
      .catch(() => {});
  }, [workspaceId]);
  const create = async () => {
    setBusy(true);
    setError(null);
    try {
      const p = await projectsApi.create({
        name,
        description,
        icon,
        accent,
        template_id: template,
        ...(workspaceId ? { workspace_id: workspaceId } : {}),
      });
      onCreated(p.project_id);
    } catch (e) {
      setError(message(e));
    } finally {
      setBusy(false);
    }
  };
  return (
    <section className="settings">
      <div className="column wide">
        <h1>New project</h1>
        <form
          className="form stack narrow-form"
          onSubmit={(e) => {
            e.preventDefault();
            void create();
          }}
        >
          <label>
            Name
            <input autoFocus maxLength={LIMITS.name} value={name} onChange={(e) => setName(e.target.value)} />
          </label>
          <label>
            Description
            <input maxLength={LIMITS.description} value={description} placeholder="Optional, one line" onChange={(e) => setDescription(e.target.value)} />
          </label>
          <LookFields icon={icon} accent={accent} onIcon={setIcon} onAccent={setAccent} />
          <label>
            Board
            <select value={template} onChange={(e) => setTemplate(e.target.value)}>
              {(templates.length ? templates : [{ template_id: "basic", name: "Basic", builtin: true, columns: [] }]).map((t) => (
                <option key={t.template_id} value={t.template_id}>
                  {t.name}
                  {t.columns.length ? ` — ${t.columns.map((c) => c.name).join(" · ")}` : ""}
                </option>
              ))}
            </select>
          </label>
          {error && <p className="error-text">{error}</p>}
          <div className="row">
            <button className="primary" disabled={busy || !name.trim()}>
              Create project
            </button>
          </div>
        </form>
      </div>
    </section>
  );
}

/** What the project's chats published (`?project=`), newest first; each
 * opens on the Artifacts page, which has the versions and actions. */
function ProjectArtifacts(props: { project: ProjectDetail; workspaceId: string | null }) {
  const [list, setList] = useState<Artifact[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const pid = props.project.project_id;
  useEffect(() => {
    artifactsApi.list({ workspace: props.workspaceId, project: pid }).then(
      (p) => setList(p.artifacts),
      (e) => setError(message(e)),
    );
  }, [pid, props.workspaceId]);
  if (error) return <p className="error-text">{error}</p>;
  if (!list) return <p className="muted">Loading…</p>;
  if (!list.length) return <p className="muted">No artifacts yet. Files agents publish from this project's agentic chats show here.</p>;
  return (
    <ul className="project-chats">
      {list.map((a) => (
        <li key={a.artifact_id}>
          <a className="session-item" href={artifactHash(a.artifact_id)}>
            <span className="ellipsis grow">{a.name}</span>
            {a.latest && <span className="muted small">v{a.latest.seq}</span>}
          </a>
          {a.latest?.status === "ready" && (
            <a className="link small" href={latestUrl(a.artifact_id)} download>
              Download
            </a>
          )}
        </li>
      ))}
    </ul>
  );
}

function Chats(props: { project: ProjectDetail; own: SessionRow[]; all: SessionRow[]; onOpen: (id: string) => void }) {
  const [error, setError] = useState<string | null>(null);
  const loose = props.all.filter((c) => !c.project_id);
  const move = (sid: string, to: string | null) => {
    setError(null);
    projectsApi.setChatProject(sid, to).catch((e) => setError(message(e)));
  };
  return (
    <div className="stack">
      {props.own.length === 0 && (
        <p className="muted">No chats in this project yet. Start one above{loose.length > 0 ? ", or add one below" : ""}.</p>
      )}
      <ul className="project-chats">
        {props.own.map((c) => (
          <li key={c.session_id}>
            <button className="session-item" onClick={() => props.onOpen(c.session_id)}>
              <MessageSquare size={15} className="muted" />
              <span className="ellipsis grow">{c.title || "New chat"}</span>
              <span className="muted small">{new Date(c.updated_ms).toLocaleDateString()}</span>
            </button>
            <button className="icon small" aria-label="Remove from project" title="Remove from project" onClick={() => move(c.session_id, null)}>
              <X size={14} />
            </button>
          </li>
        ))}
      </ul>
      {loose.length > 0 && (
        <label className="field-inline">
          <span className="muted small">Add a chat</span>
          <select value="" onChange={(e) => e.target.value && move(e.target.value, props.project.project_id)}>
            <option value="">Choose one of your chats…</option>
            {loose.map((c) => (
              <option key={c.session_id} value={c.session_id}>
                {c.title || "New chat"}
              </option>
            ))}
          </select>
        </label>
      )}
      <p className="field-hint">Each person sees only their own chats here. The instructions, memory and board are shared.</p>
      {error && <p className="error-text">{error}</p>}
    </div>
  );
}

function BoardView(props: { project: ProjectDetail; chats: SessionRow[]; version: number; onOpen: (id: string) => void }) {
  const pid = props.project.project_id;
  const manage = props.project.can_manage;
  const [board, setBoard] = useState<Board | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [editing, setEditing] = useState<string | null>(null);
  const [dragging, setDragging] = useState<string | null>(null);
  useEffect(() => {
    projectsApi
      .board(pid)
      .then(setBoard)
      .catch((e) => setError(message(e)));
  }, [pid, props.version]);
  /** Run a change; the answer is the whole new board. */
  const run = useCallback((p: Promise<Board>) => {
    setError(null);
    p.then(setBoard).catch((e) => setError(message(e)));
  }, []);
  if (!board) return error ? <p className="error-text">{error}</p> : <p className="muted">Loading…</p>;
  const empty = board.columns.every((c) => c.cards.length === 0);

  const drop = (e: DragEvent, col: BoardColumn, before: string | null) => {
    e.preventDefault();
    e.stopPropagation();
    const card = dragging ?? e.dataTransfer.getData("text/plain");
    setDragging(null);
    if (card) run(projectsApi.updateCard(pid, card, { column_id: col.column_id, position: dropIndex(col, card, before) }));
  };

  return (
    <div className="stack">
      <div className="board">
        {board.columns.map((col, i) => (
          <section
            key={col.column_id}
            className={`board-col ${col.done ? "done" : ""}`}
            onDragOver={(e) => manage && e.preventDefault()}
            onDrop={(e) => drop(e, col, null)}
          >
            <ColumnHead
              col={col}
              manage={manage}
              first={i === 0}
              last={i === board.columns.length - 1}
              onChange={(b) => run(projectsApi.updateColumn(pid, col.column_id, b))}
              onMove={(d) => run(projectsApi.updateColumn(pid, col.column_id, { position: i + d }))}
              onDelete={() => run(projectsApi.deleteColumn(pid, col.column_id))}
            />
            <ul className="board-cards">
              {col.cards.map((card) =>
                editing === card.card_id ? (
                  <CardEditor
                    key={card.card_id}
                    card={card}
                    columns={board.columns}
                    chats={props.chats}
                    onSave={(b) => {
                      run(projectsApi.updateCard(pid, card.card_id, b));
                      setEditing(null);
                    }}
                    onDelete={() => {
                      run(projectsApi.deleteCard(pid, card.card_id));
                      setEditing(null);
                    }}
                    onClose={() => setEditing(null)}
                  />
                ) : (
                  <li
                    key={card.card_id}
                    className={`board-card ${dragging === card.card_id ? "dragging" : ""}`}
                    draggable={manage}
                    onDragStart={(e) => {
                      e.dataTransfer.setData("text/plain", card.card_id);
                      setDragging(card.card_id);
                    }}
                    onDragEnd={() => setDragging(null)}
                    onDragOver={(e) => manage && e.preventDefault()}
                    onDrop={(e) => drop(e, col, card.card_id)}
                  >
                    <button className="card-open" disabled={!manage} onClick={() => setEditing(card.card_id)}>
                      <span className="card-title">{card.title}</span>
                      {card.notes && <span className="muted small ellipsis">{card.notes.split("\n")[0]}</span>}
                    </button>
                    {card.chat && card.chat !== "private" && (
                      <button className="link small" onClick={() => props.onOpen((card.chat as { session_id: string }).session_id)}>
                        <MessageSquare size={12} /> {card.chat.title || "Chat"}
                      </button>
                    )}
                    {card.chat === "private" && <span className="muted small">Linked to someone's chat</span>}
                  </li>
                ),
              )}
            </ul>
            {manage && <AddLine placeholder="Add a card" max={LIMITS.title} onAdd={(title) => run(projectsApi.addCard(pid, { column_id: col.column_id, title }))} />}
          </section>
        ))}
        {manage && (
          <section className="board-col add">
            <AddLine placeholder="Add a column" max={LIMITS.column} onAdd={(name) => run(projectsApi.addColumn(pid, name))} />
          </section>
        )}
      </div>
      {error && <p className="error-text">{error}</p>}
      {manage && <Templates projectId={pid} workspaceId={props.project.workspace_id} empty={empty} onBoard={setBoard} />}
      <p className="field-hint">The open cards (not those in a done column) go to the model with every message in this project.</p>
    </div>
  );
}

function ColumnHead(props: {
  col: BoardColumn;
  manage: boolean;
  first: boolean;
  last: boolean;
  onChange: (b: { name?: string; done?: boolean }) => void;
  onMove: (d: -1 | 1) => void;
  onDelete: () => void;
}) {
  const [name, setName] = useState<string | null>(null);
  const { col } = props;
  if (!props.manage) return <h3 className="board-col-head">{col.name}</h3>;
  return (
    <div className="board-col-head">
      {name === null ? (
        <button className="link grow" title="Rename" onClick={() => setName(col.name)}>
          {col.name} <span className="muted small">{col.cards.length}</span>
        </button>
      ) : (
        <input
          autoFocus
          maxLength={LIMITS.column}
          value={name}
          onChange={(e) => setName(e.target.value)}
          onBlur={() => {
            if (name.trim() && name !== col.name) props.onChange({ name });
            setName(null);
          }}
          onKeyDown={(e) => {
            if (e.key === "Enter") (e.target as HTMLInputElement).blur();
            if (e.key === "Escape") setName(null);
          }}
        />
      )}
      {col.done && <span className="muted small">done</span>}
      <span className="col-tools">
        <label className="check small" title="Cards here count as finished">
          <input type="checkbox" checked={col.done} onChange={(e) => props.onChange({ done: e.target.checked })} /> done
        </label>
        <button className="icon small" aria-label="Move left" disabled={props.first} onClick={() => props.onMove(-1)}>
          <ArrowLeft size={13} />
        </button>
        <button className="icon small" aria-label="Move right" disabled={props.last} onClick={() => props.onMove(1)}>
          <ArrowRight size={13} />
        </button>
        {col.cards.length === 0 && (
          <button className="icon small" aria-label="Delete column" title="Delete column" onClick={props.onDelete}>
            <Trash2 size={13} />
          </button>
        )}
      </span>
    </div>
  );
}

function CardEditor(props: {
  card: BoardCard;
  columns: BoardColumn[];
  chats: SessionRow[];
  onSave: (b: { title: string; notes: string; column_id: string; session_id?: string | null }) => void;
  onDelete: () => void;
  onClose: () => void;
}) {
  const { card } = props;
  const [title, setTitle] = useState(card.title);
  const [notes, setNotes] = useState(card.notes);
  const [column, setColumn] = useState(card.column_id);
  const linked = card.chat && card.chat !== "private" ? card.chat.session_id : null;
  const [chat, setChat] = useState(linked ?? "");
  return (
    <li className="board-card editing">
      <form
        className="form stack"
        onSubmit={(e) => {
          e.preventDefault();
          // A link to someone else's chat stays as it is.
          props.onSave({ title, notes, column_id: column, ...(card.chat === "private" ? {} : { session_id: chat || null }) });
        }}
      >
        <input autoFocus maxLength={LIMITS.title} value={title} aria-label="Title" onChange={(e) => setTitle(e.target.value)} />
        <textarea rows={4} maxLength={LIMITS.notes} value={notes} placeholder="Notes" aria-label="Notes" onChange={(e) => setNotes(e.target.value)} />
        <label>
          Column
          <select value={column} onChange={(e) => setColumn(e.target.value)}>
            {props.columns.map((c) => (
              <option key={c.column_id} value={c.column_id}>
                {c.name}
              </option>
            ))}
          </select>
        </label>
        {card.chat !== "private" && (
          <label>
            Chat
            <select value={chat} onChange={(e) => setChat(e.target.value)}>
              <option value="">None</option>
              {props.chats.map((c) => (
                <option key={c.session_id} value={c.session_id}>
                  {c.title || "New chat"}
                </option>
              ))}
            </select>
          </label>
        )}
        <div className="row">
          <button className="primary" disabled={!title.trim()}>
            Save
          </button>
          <button type="button" onClick={props.onClose}>
            Cancel
          </button>
          <span className="grow" />
          <button type="button" className="danger" onClick={props.onDelete}>
            Delete
          </button>
        </div>
      </form>
    </li>
  );
}

/** One input that adds an item on Enter. */
function AddLine(props: { placeholder: string; max: number; onAdd: (text: string) => void }) {
  const [text, setText] = useState("");
  return (
    <input
      className="add-line"
      placeholder={props.placeholder}
      aria-label={props.placeholder}
      maxLength={props.max}
      value={text}
      onChange={(e) => setText(e.target.value)}
      onKeyDown={(e) => {
        if (e.key === "Enter" && text.trim()) {
          props.onAdd(text.trim());
          setText("");
        }
      }}
    />
  );
}

/** Start an empty board from a template; save these columns as one. */
function Templates(props: { projectId: string; workspaceId: string; empty: boolean; onBoard: (b: Board) => void }) {
  const [templates, setTemplates] = useState<BoardTemplate[]>([]);
  const [name, setName] = useState("");
  const [error, setError] = useState<string | null>(null);
  const { workspaceId } = props;
  const load = useCallback(() => {
    projectsApi
      .templates(workspaceId)
      .then((r) => setTemplates(r.templates))
      .catch(() => {});
  }, [workspaceId]);
  useEffect(load, [load]);
  const act = (p: Promise<unknown>) => {
    setError(null);
    p.then(load).catch((e) => setError(message(e)));
  };
  const saved = templates.filter((t) => !t.builtin);
  return (
    <details className="templates">
      <summary className="muted small">Board templates</summary>
      <div className="stack">
        {props.empty && (
          <label className="field-inline">
            <span className="muted small">Start from</span>
            <select
              value=""
              onChange={(e) =>
                e.target.value &&
                act(projectsApi.applyTemplate(props.projectId, e.target.value).then(props.onBoard))
              }
            >
              <option value="">Choose a template…</option>
              {templates.map((t) => (
                <option key={t.template_id} value={t.template_id}>
                  {t.name}
                </option>
              ))}
            </select>
          </label>
        )}
        <form
          className="row"
          onSubmit={(e) => {
            e.preventDefault();
            act(projectsApi.saveTemplate(name, props.projectId).then(() => setName("")));
          }}
        >
          <input maxLength={LIMITS.name} value={name} placeholder="Template name" aria-label="Template name" onChange={(e) => setName(e.target.value)} />
          <button disabled={!name.trim()}>Save these columns as a template</button>
        </form>
        {saved.length > 0 && (
          <ul className="facts">
            {saved.map((t) => (
              <li key={t.template_id} className="fact row">
                <span className="grow">
                  {t.name} <span className="muted small">{t.columns.map((c) => c.name).join(" · ")}</span>
                </span>
                <button className="icon small" aria-label={`Delete ${t.name}`} onClick={() => act(projectsApi.deleteTemplate(t.template_id))}>
                  <Trash2 size={13} />
                </button>
              </li>
            ))}
          </ul>
        )}
        {error && <p className="error-text">{error}</p>}
      </div>
    </details>
  );
}

function ProjectMemory(props: { project: ProjectDetail; version: number; onOpen: (id: string) => void }) {
  const pid = props.project.project_id;
  const manage = props.project.can_manage;
  const [facts, setFacts] = useState<MemoryFact[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [text, setText] = useState("");
  const [editing, setEditing] = useState<{ id: string; text: string } | null>(null);
  const load = useCallback(() => {
    projectsApi
      .memories(pid)
      .then((r) => setFacts(r.memories))
      .catch((e) => setError(message(e)));
  }, [pid]);
  useEffect(load, [load, props.version]);
  const act = (p: Promise<unknown>) => {
    setError(null);
    p.then(load).catch((e) => setError(message(e)));
  };
  if (!facts) return error ? <p className="error-text">{error}</p> : <p className="muted">Loading…</p>;
  const proposed = facts.filter((f) => f.status === "proposed");
  const active = facts.filter((f) => f.status === "active");
  return (
    <div className="stack">
      <p className="field-hint">
        What the model recalls in this project's chats, shared with everyone who can see the project. The model can propose facts
        from your chats; only you see a proposal until you keep it.
      </p>
      {proposed.length > 0 && (
        <>
          <h3>From your chats, to review</h3>
          <ul className="facts">
            {proposed.map((f) => (
              <li key={f.id} className="fact proposed">
                <div>{f.text}</div>
                <div className="row fact-meta small">
                  {manage && (
                    <button className="link" onClick={() => act(projectsApi.updateMemory(pid, f.id, { status: "active" }))}>
                      Keep for the project
                    </button>
                  )}
                  <button className="link" onClick={() => act(projectsApi.deleteMemory(pid, f.id))}>
                    Discard
                  </button>
                  {f.source_session && (
                    <button className="link muted" onClick={() => props.onOpen(f.source_session!)}>
                      Source chat
                    </button>
                  )}
                </div>
              </li>
            ))}
          </ul>
        </>
      )}
      {active.length === 0 && <p className="muted">Nothing remembered for this project yet.</p>}
      <ul className="facts">
        {active.map((f) =>
          editing?.id === f.id ? (
            <li key={f.id} className="fact">
              <form
                className="row"
                onSubmit={(e) => {
                  e.preventDefault();
                  act(projectsApi.updateMemory(pid, f.id, { text: editing.text }).then(() => setEditing(null)));
                }}
              >
                <input className="grow" autoFocus value={editing.text} aria-label="Fact" onChange={(e) => setEditing({ id: f.id, text: e.target.value })} />
                <button className="primary">Save</button>
                <button type="button" onClick={() => setEditing(null)}>
                  Cancel
                </button>
              </form>
            </li>
          ) : (
            <li key={f.id} className="fact">
              <div>{f.text}</div>
              {manage && (
                <div className="row fact-meta small">
                  <button className="link" onClick={() => setEditing({ id: f.id, text: f.text })}>
                    Edit
                  </button>
                  <button className="link" onClick={() => act(projectsApi.deleteMemory(pid, f.id))}>
                    Forget
                  </button>
                </div>
              )}
            </li>
          ),
        )}
      </ul>
      {manage && (
        <form
          className="row"
          onSubmit={(e) => {
            e.preventDefault();
            act(projectsApi.addMemory(pid, text).then(() => setText("")));
          }}
        >
          <input className="grow" value={text} placeholder="Add a fact about this project" aria-label="New fact" onChange={(e) => setText(e.target.value)} />
          <button disabled={!text.trim()}>Add</button>
        </form>
      )}
      {error && <p className="error-text">{error}</p>}
    </div>
  );
}

function ProjectSettings(props: {
  project: ProjectDetail;
  nodes: NodeInfo[];
  onSaved: (p: ProjectDetail) => void;
  onDeleted: () => void;
}) {
  const p = props.project;
  const [draft, setDraft] = useState(p);
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);
  const disabled = !p.can_manage;
  const set = (patch: Partial<ProjectDetail>) => {
    setSaved(false);
    setDraft((d) => ({ ...d, ...patch }));
  };
  const save = async () => {
    setError(null);
    try {
      const { name, description, icon, accent, instructions, settings } = draft;
      props.onSaved(await projectsApi.update(p.project_id, { name, description, icon, accent, instructions, settings }));
      setSaved(true);
    } catch (e) {
      setError(message(e));
    }
  };
  const remove = async () => {
    if (!confirm(`Delete "${p.name}"? Its chats stay (outside any project); its board and memory are deleted.`)) return;
    try {
      await projectsApi.remove(p.project_id);
      props.onDeleted();
    } catch (e) {
      setError(message(e));
    }
  };
  const workers = props.nodes.filter((n) => n.profile === "agentic");
  return (
    <form
      className="form stack narrow-form"
      onSubmit={(e) => {
        e.preventDefault();
        void save();
      }}
    >
      <label>
        Name
        <input disabled={disabled} maxLength={LIMITS.name} value={draft.name} onChange={(e) => set({ name: e.target.value })} />
      </label>
      <label>
        Description
        <input disabled={disabled} maxLength={LIMITS.description} value={draft.description} onChange={(e) => set({ description: e.target.value })} />
      </label>
      <LookFields
        disabled={disabled}
        icon={draft.icon}
        accent={draft.accent}
        onIcon={(icon) => set({ icon })}
        onAccent={(accent) => set({ accent })}
      />
      <label>
        Instructions
        <textarea
          disabled={disabled}
          rows={8}
          maxLength={LIMITS.instructions}
          value={draft.instructions}
          placeholder="What the model should know and follow in every chat of this project."
          onChange={(e) => set({ instructions: e.target.value })}
        />
        <span className="field-hint">
          {draft.instructions.length} / {LIMITS.instructions} · sent with every message in this project's chats.
        </span>
      </label>
      <label className="check">
        <input type="checkbox" disabled checked={draft.settings.workspace_memory} readOnly /> Use the workspace's memory too
      </label>
      <span className="field-hint">Workspace memory comes with team workspaces; this project will use it when it exists.</span>
      <label className="check">
        <input
          type="checkbox"
          disabled={disabled}
          checked={draft.settings.members_activate_facts}
          onChange={(e) => set({ settings: { ...draft.settings, members_activate_facts: e.target.checked } })}
        />{" "}
        Members may confirm project facts
      </label>
      <span className="field-hint">
        Off: only managers make a proposed fact part of the project's memory, which every member's agent reads.
      </span>
      <h3>Agentic chats</h3>
      <label>
        Worker node
        <select
          disabled={disabled}
          value={draft.settings.node_id ?? ""}
          onChange={(e) => set({ settings: { ...draft.settings, node_id: e.target.value || null } })}
        >
          <option value="">The bar's choice</option>
          {workers.map((n) => (
            <option key={n.node_id} value={n.node_id}>
              {n.hostname || n.node_id}
            </option>
          ))}
        </select>
      </label>
      <label>
        Folder
        <input
          disabled={disabled}
          className="mono"
          value={draft.settings.cwd ?? ""}
          placeholder="/path/on/the/node"
          onChange={(e) => set({ settings: { ...draft.settings, cwd: e.target.value || null } })}
        />
        <span className="field-hint">Filled in when you start an agentic chat here; the node's own rules still apply.</span>
      </label>
      {error && <p className="error-text">{error}</p>}
      {!disabled && (
        <div className="row">
          <button className="primary">Save</button>
          {saved && <span className="muted small">Saved</span>}
          <span className="grow" />
          <button type="button" className="danger" onClick={() => void remove()}>
            Delete project
          </button>
        </div>
      )}
    </form>
  );
}

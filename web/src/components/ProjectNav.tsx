// The sidebar's scroll region (docs/sidebar-search.md, the owner's sketch of
// 2026-11-16): Projects as boxed groups that hold their chats, then the chats
// in no project; a sub-agent's chat nests under its parent. Collapse state is
// this browser's (localStorage, inside try/catch).
import { ChevronRight, Cloud, Plus } from "lucide-react";
import { type ReactNode, useId, useState } from "react";
import { ACCENTS } from "../lib/appearance";
import { t } from "../lib/i18n";
import { ProjectIcon } from "../lib/projectIcons";
import type { ProjectSummary } from "../lib/projects";
import type { Route } from "../lib/routes";
import { shownName } from "../md/hiddenChars";
import type { NodeInfo, SessionRow } from "../types";
import { NavLink } from "./NavLink";

const OPEN_KEY = "harness.sidebarSections";
/** Closed groups: `projects` (the section) and project ids. */
type Open = Record<string, boolean>;

function readOpen(): Open {
  try {
    const v: unknown = JSON.parse(localStorage.getItem(OPEN_KEY) ?? "{}");
    return v && typeof v === "object" ? (v as Open) : {};
  } catch {
    return {};
  }
}

/** A node's colour: one of the accent presets (themed for light and dark),
 * picked by a hash of its id, so it is the same for everyone, everywhere. */
export function nodeHue(id: string): string {
  let h = 0;
  for (let i = 0; i < id.length; i++) h = (h * 31 + id.charCodeAt(i)) | 0;
  // Graphite (the last) is too close to the row's own grey.
  return `var(--ac-${ACCENTS[(h >>> 0) % (ACCENTS.length - 1)]}-2)`;
}

/** The first `cap` rows, then the rest on request. */
function Capped<T>(props: { items: T[]; cap: number; row: (x: T) => ReactNode }) {
  const [all, setAll] = useState(false);
  const more = props.items.length - props.cap;
  return (
    <>
      {(all ? props.items : props.items.slice(0, props.cap)).map(props.row)}
      {more > 0 && (
        <li>
          <button type="button" className="link small show-more" onClick={() => setAll((v) => !v)}>
            {all ? t("nav.showFewer") : t("nav.showMore", { n: String(more) })}
          </button>
        </li>
      )}
    </>
  );
}

interface Ctx {
  kids: Map<string, SessionRow[]>;
  nodes: Map<string, NodeInfo>;
  selected: string | null;
  onNavigate: (r: Route) => void;
}

/** An agentic chat's node: a square in the node's colour, named on hover
 * (title), on focus of the row (CSS) and to assistive tech (label). */
function NodeMark({ id, node }: { id: string; node?: NodeInfo }) {
  // The hostname is the node's own word: hidden characters show (W-3).
  const name = t("nav.runsOn", { name: shownName(node ? (node.embedded ? "local" : node.hostname) : id) }) + (node?.connected ? "" : ", offline");
  return (
    <span className={`node-mark${node?.connected ? "" : " off"}`} role="img" aria-label={name} title={name} style={{ "--node": nodeHue(id) } as object}>
      <Cloud size={12} aria-hidden />
    </span>
  );
}

function Chat({ s, c }: { s: SessionRow; c: Ctx }) {
  const kids = c.kids.get(s.session_id);
  const sel = s.session_id === c.selected;
  return (
    <li>
      <NavLink to={{ kind: "session", id: s.session_id }} current={sel} onNavigate={c.onNavigate} className="session-item" title={shownName(s.title)}>
        {s.running && <span className="dot running" title="running" />}
        {!s.running && !!s.background && <span className="dot background" title={`${s.background} background command(s) running`} />}
        {!s.running && s.unread && !sel && <span className="dot unseen" title={t("nav.unread")} aria-label={t("nav.unread")} role="img" />}
        <span className="ellipsis grow">{shownName(s.title) || t("session.untitled")}</span>
        {s.profile === "agentic" && <NodeMark id={s.node_id} node={c.nodes.get(s.node_id)} />}
      </NavLink>
      {kids && <Chats list={kids} c={c} cap={3} sub />}
    </li>
  );
}

function Chats({ list, c, cap = Infinity, sub }: { list: SessionRow[]; c: Ctx; cap?: number; sub?: boolean }) {
  return (
    <ul className={sub ? "chats sub" : "chats"}>
      <Capped items={list} cap={cap} row={(s) => <Chat key={s.session_id} s={s} c={c} />} />
    </ul>
  );
}

/** A chevron that opens and closes `body`. */
function Toggle(props: { open: boolean; body: string; label?: string; onToggle: () => void; children?: ReactNode }) {
  return (
    <button type="button" className="section-toggle" aria-expanded={props.open} aria-controls={props.body} aria-label={props.label} onClick={props.onToggle}>
      {/* Turned down by CSS while open. */}
      <ChevronRight size={12} />
      {props.children}
    </button>
  );
}

function Plus14(props: { label: string; onClick: () => void }) {
  return (
    <button type="button" className="icon small" aria-label={props.label} title={props.label} onClick={props.onClick}>
      <Plus size={14} />
    </button>
  );
}

function Box(props: { p: ProjectSummary; chats: SessionRow[]; open: boolean; onToggle: () => void; route: Route; c: Ctx }) {
  const { p, c } = props;
  const body = useId();
  return (
    <li className="project-box">
      <div className="section-row">
        <Toggle open={props.open} body={body} label={p.name} onToggle={props.onToggle} />
        <NavLink
          to={{ kind: "project", id: p.project_id }}
          current={props.route.kind === "project" && props.route.id === p.project_id}
          onNavigate={c.onNavigate}
          className="session-item grow"
          title={p.description || p.name}
        >
          <ProjectIcon icon={p.icon} accent={p.accent} />
          <span className="ellipsis grow">{p.name}</span>
        </NavLink>
        {/* A project's chat starts on its page (its composer files it there). */}
        <Plus14 label={t("nav.newIn", { name: p.name })} onClick={() => c.onNavigate({ kind: "project", id: p.project_id })} />
      </div>
      {props.open && (
        <div id={body}>{props.chats.length ? <Chats list={props.chats} c={c} cap={5} /> : <div className="muted pad small">{t("nav.noChats")}</div>}</div>
      )}
    </li>
  );
}

export function SidebarLists(props: {
  route: Route;
  onNavigate: (r: Route) => void;
  /** The workspace's chats, newest first, sub-agents' included. */
  sessions: SessionRow[];
  nodes: NodeInfo[];
  projects: ProjectSummary[];
  canCreateProject: boolean;
}) {
  const [open, setOpen] = useState(readOpen);
  const body = useId();
  const toggle = (id: string) =>
    setOpen((o) => {
      const next = { ...o, [id]: o[id] === false };
      try {
        localStorage.setItem(OPEN_KEY, JSON.stringify(next));
      } catch {
        /* storage unavailable */
      }
      return next;
    });

  const rows = props.sessions.filter((s) => !s.incognito);
  const ids = new Set(rows.map((s) => s.session_id));
  const kids = new Map<string, SessionRow[]>();
  for (const s of rows) if (s.parent_id && ids.has(s.parent_id)) kids.set(s.parent_id, [...(kids.get(s.parent_id) ?? []), s]);
  // Top level: chats without a parent; a sub-agent whose parent is not listed stays hidden.
  const top = rows.filter((s) => !s.parent_id);
  const projects = [...props.projects].sort((a, b) => b.updated_ms - a.updated_ms);
  const listed = new Set(projects.map((p) => p.project_id));
  const loose = top.filter((s) => !s.project_id || !listed.has(s.project_id));
  const c: Ctx = {
    kids,
    nodes: new Map(props.nodes.map((n) => [n.node_id, n])),
    selected: props.route.kind === "session" ? props.route.id : null,
    onNavigate: props.onNavigate,
  };
  const sectionOpen = open.projects !== false;

  return (
    <div className="sidebar-scroll">
      <section className="side-section">
        <div className="section-label section-row">
          <Toggle open={sectionOpen} body={body} onToggle={() => toggle("projects")}>
            <span>{t("nav.projects")}</span>
          </Toggle>
          <span className="grow" />
          {props.canCreateProject && <Plus14 label={t("nav.newProject")} onClick={() => props.onNavigate({ kind: "project", id: "new" })} />}
        </div>
        {sectionOpen && (
          <ul id={body} className="projects">
            {projects.length === 0 && <li className="muted pad small">{t("nav.noProjects")}</li>}
            <Capped
              items={projects}
              cap={5}
              row={(p) => (
                <Box
                  key={p.project_id}
                  p={p}
                  chats={top.filter((s) => s.project_id === p.project_id)}
                  open={open[p.project_id] !== false}
                  onToggle={() => toggle(p.project_id)}
                  route={props.route}
                  c={c}
                />
              )}
            />
          </ul>
        )}
      </section>
      {top.length === 0 && <div className="muted pad small">{t("nav.noChats")}</div>}
      <Chats list={loose} c={c} />
    </div>
  );
}

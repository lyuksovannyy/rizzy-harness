// The sidebar's scroll region (docs/sidebar-search.md, the owner's sketch of
// 2026-11-16): Projects as boxed groups that hold their chats, then the chats
// in no project; a sub-agent's chat nests under its parent. Collapse state is
// this browser's (localStorage, inside try/catch). The open chat is never lost:
// a cap shows it in its place, and a closed group's header carries its mark.
import { ChevronRight, Cloud, Plus } from "lucide-react";
import { type ReactNode, useId, useMemo, useState, useSyncExternalStore } from "react";
import { ACCENTS } from "../lib/appearance";
import { t } from "../lib/i18n";
import { ProjectIcon } from "../lib/projectIcons";
import { type PendingProject, pendingProjects, projectsApi, type ProjectSummary } from "../lib/projects";
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

/** The first `cap` rows and any row `keep` asks for (the open chat, its
 * parents, its project) in its place; the rest on request. */
function Capped<T>(props: { items: T[]; cap: number; keep: (x: T) => boolean; row: (x: T) => ReactNode }) {
  const [all, setAll] = useState(false);
  const first = props.items.filter((x, i) => i < props.cap || props.keep(x));
  const more = props.items.length - first.length;
  return (
    <>
      {(all ? props.items : first).map(props.row)}
      {(more > 0 || all) && (
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
  /** The open chat and its parents. */
  path: Set<string>;
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
    // `role`: Safari drops a styled list's semantics.
    <ul role="list" className={sub ? "chats sub" : "chats"}>
      <Capped items={list} cap={cap} keep={(s) => c.path.has(s.session_id)} row={(s) => <Chat key={s.session_id} s={s} c={c} />} />
    </ul>
  );
}

/** What a closed group's header says about the chats inside it (sub-agents
 * included): the open chat is there, one runs, one finished unseen. */
function Marks({ list, c }: { list: SessionRow[]; c: Ctx }) {
  let here = false;
  let run = false;
  let unseen = false;
  const walk = (xs: SessionRow[]) => {
    for (const s of xs) {
      here ||= c.path.has(s.session_id);
      run ||= s.running;
      unseen ||= !!s.unread && !s.running && s.session_id !== c.selected;
      walk(c.kids.get(s.session_id) ?? []);
    }
  };
  walk(list);
  const dot = (cls: string, label: string) => <span className={`dot ${cls}`} role="img" aria-label={label} title={label} />;
  return (
    <>
      {here && dot("here", t("nav.holdsOpen"))}
      {run ? dot("running", t("nav.holdsRunning")) : unseen && dot("unseen", t("nav.unread"))}
    </>
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
  // Another member may have named it: hidden characters show (W-3).
  const name = shownName(p.name);
  const here = !props.open && props.chats.some((s) => c.path.has(s.session_id));
  return (
    <li className="project-box">
      <div className="section-row">
        <Toggle open={props.open} body={body} label={t(props.open ? "nav.collapseX" : "nav.expandX", { name })} onToggle={props.onToggle} />
        <NavLink
          to={{ kind: "project", id: p.project_id }}
          current={props.route.kind === "project" && props.route.id === p.project_id}
          onNavigate={c.onNavigate}
          className={`session-item grow${here ? " active" : ""}`}
          title={shownName(p.description) || name}
        >
          <ProjectIcon icon={p.icon} accent={p.accent} />
          <span className="ellipsis grow">{name}</span>
          {!props.open && <Marks list={props.chats} c={c} />}
        </NavLink>
        {/* A project's chat starts on its page (its composer files it there);
            on that page already, the composer takes the focus. */}
        <Plus14
          label={t("nav.newIn", { name })}
          onClick={() => {
            c.onNavigate({ kind: "project", id: p.project_id });
            setTimeout(() => document.querySelector<HTMLElement>(".project-page .composer textarea")?.focus());
          }}
        />
      </div>
      {props.open && (
        <div id={body}>{props.chats.length ? <Chats list={props.chats} c={c} cap={5} /> : <div className="muted pad small">{t("nav.noChats")}</div>}</div>
      )}
    </li>
  );
}

/** Projects whose folder is being cloned (X.27): shown, never a link (no
 * chat can start in one); a failed one, its starter's only, with the reason
 * and Dismiss. */
function Pending({ list }: { list: PendingProject[] }) {
  // What the server said when a Dismiss was refused (by project).
  const [refused, setRefused] = useState<Record<string, string>>({});
  const dismiss = (id: string) =>
    projectsApi.dismissClone(id).then(
      () => setRefused(({ [id]: _, ...rest }) => rest),
      (e: unknown) => setRefused((r) => ({ ...r, [id]: e instanceof Error ? e.message : String(e) })),
    );
  return list.map((p) => {
    const what = shownName(`${p.source.host}/${p.source.full_name}`);
    const why = p.reason ? shownName(p.reason) : "";
    return (
      <li key={p.project_id} className="pending-project muted small" title={why || what}>
        <span className="ellipsis grow">
          {shownName(p.name)} · {why ? `clone failed: ${why}` : `cloning ${what}…`}
        </span>
        {why && p.mine && (
          <button type="button" className="link" onClick={() => void dismiss(p.project_id)}>
            Dismiss
          </button>
        )}
        {refused[p.project_id] && (
          <span className="error-text" role="alert">
            {shownName(refused[p.project_id]!)}
          </span>
        )}
      </li>
    );
  });
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
  const pending = useSyncExternalStore(pendingProjects.sub, pendingProjects.get);
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

  const tree = useMemo(() => {
    const rows = props.sessions.filter((s) => !s.incognito);
    const byId = new Map(rows.map((s) => [s.session_id, s]));
    const kids = new Map<string, SessionRow[]>();
    for (const s of rows) {
      if (!s.parent_id || !byId.has(s.parent_id)) continue;
      const list = kids.get(s.parent_id);
      if (list) list.push(s);
      else kids.set(s.parent_id, [s]);
    }
    // Top level: chats without a parent; a sub-agent whose parent is not listed stays hidden.
    const top = rows.filter((s) => !s.parent_id);
    const projects = [...props.projects].sort((a, b) => b.updated_ms - a.updated_ms);
    const inProject = new Map<string, SessionRow[]>(projects.map((p) => [p.project_id, []]));
    const loose: SessionRow[] = [];
    for (const s of top) (s.project_id ? inProject.get(s.project_id) : undefined)?.push(s) ?? loose.push(s);
    return { byId, kids, projects, inProject, loose, none: top.length === 0 };
  }, [props.sessions, props.projects]);

  const selected = props.route.kind === "session" ? props.route.id : null;
  // The open chat and its parents, up to a top-level chat; nothing for a
  // chat the sidebar does not list (incognito, or a parent missing or in a cycle).
  const path = new Set<string>();
  for (let s = selected ? tree.byId.get(selected) : undefined; s && !path.has(s.session_id); s = s.parent_id ? tree.byId.get(s.parent_id) : undefined) {
    path.add(s.session_id);
    if (!s.parent_id) break;
  }
  const c: Ctx = { kids: tree.kids, nodes: new Map(props.nodes.map((n) => [n.node_id, n])), selected, path, onNavigate: props.onNavigate };
  const sectionOpen = open.projects !== false;
  const projectChats = sectionOpen ? [] : [...tree.inProject.values()].flat();
  const sectionHere = projectChats.some((s) => path.has(s.session_id));

  return (
    <div className="sidebar-scroll">
      <section className="side-section">
        <div className={`section-label section-row${sectionHere ? " active" : ""}`}>
          <Toggle open={sectionOpen} body={body} onToggle={() => toggle("projects")}>
            <span>{t("nav.projects")}</span>
          </Toggle>
          {!sectionOpen && <Marks list={projectChats} c={c} />}
          <span className="grow" />
          {props.canCreateProject && <Plus14 label={t("nav.newProject")} onClick={() => props.onNavigate({ kind: "project", id: "new" })} />}
        </div>
        {sectionOpen && (
          <ul id={body} role="list" className="projects">
            {tree.projects.length === 0 && !pending.length && <li className="muted pad small">{t("nav.noProjects")}</li>}
            <Pending list={pending} />
            <Capped
              items={tree.projects}
              cap={5}
              keep={(p) => tree.inProject.get(p.project_id)!.some((s) => path.has(s.session_id))}
              row={(p) => (
                <Box
                  key={p.project_id}
                  p={p}
                  chats={tree.inProject.get(p.project_id)!}
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
      {tree.none && <div className="muted pad small">{t("nav.noChats")}</div>}
      <Chats list={tree.loose} c={c} />
    </div>
  );
}

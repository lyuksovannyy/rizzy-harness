// The sidebar (docs/sidebar-search.md): a header with Search and Collapse,
// the areas the viewer can use here, Projects and Recent in one scroll
// region, and the account button. Collapsed (`rail`) it is a column of
// icons. Every row is a link; the account menu is a lazy chunk.
import {
  Bot,
  CalendarClock,
  ChevronRight,
  Images,
  Loader2,
  Menu,
  Package,
  PanelLeftClose,
  Plug,
  Plus,
  Search,
  SquarePen,
  WifiOff,
} from "lucide-react";
import { lazy, type ReactNode, Suspense, useCallback, useId, useRef, useState } from "react";
import type { ConnState } from "../live";
import { APP_TITLE } from "../lib/brand";
import { t } from "../lib/i18n";
import type { ProjectSummary } from "../lib/projects";
import type { Route } from "../lib/routes";
import { accountLine, workspaceName } from "../lib/workspaces";
import type { Features, Me, NodeInfo, SessionRow, WorkspaceList } from "../types";
import type { MenuView } from "./AccountMenu";
import { NavLink } from "./NavLink";
import { NodeBadge } from "./NodeBadge";
import { ChatProjectMark, ProjectRows } from "./ProjectNav";

// Imported ahead of the first click (pointer, focus or touch on the
// button), so opening the menu never waits on the network.
const loadMenu = () => import("./AccountMenu");
const AccountMenu = lazy(loadMenu);

const SECTIONS_KEY = "harness.sidebarSections";
type SectionId = "projects" | "recent";
type Sections = Record<SectionId, boolean>;

/** Which sections are open; both until the user closes one. */
function readSections(): Sections {
  try {
    const v = JSON.parse(localStorage.getItem(SECTIONS_KEY) ?? "{}") as Partial<Sections>;
    return { projects: v.projects !== false, recent: v.recent !== false };
  } catch {
    return { projects: true, recent: true };
  }
}

export interface SidebarProps {
  route: Route;
  onNavigate: (r: Route) => void;
  me: Me;
  /** The name the user goes by (preferences), else the username. */
  displayName: string;
  /** Recent: the workspace's listed chats, newest first. */
  sessions: SessionRow[];
  nodes: NodeInfo[];
  conn: ConnState;
  /** For the account menu's workspaces; null until loaded. */
  workspaces: WorkspaceList | null;
  /** Which areas show here (`show`). */
  features: Features;
  /** Routine results not seen yet in this workspace (the dot on Routines). */
  routineUnseen: number;
  /** Switches workspace; rejects with what went wrong. */
  onSwitch: (id: string) => Promise<void>;
  onLogout: () => void;
  onSearch: () => void;
  /** The pointer or focus is on Search: load the panel's chunk. */
  onSearchIntent: () => void;
  /** Collapse (or, as the rail, expand; in the drawer, close). */
  onCollapse: () => void;
  /** The collapsed icon column. */
  rail?: boolean;
  projects: ProjectSummary[];
  canCreateProject: boolean;
}

interface Entry {
  key: string;
  to: Route;
  icon: ReactNode;
  label: string;
  /** The accessible name, when it says more than the label. */
  name?: string;
  current: boolean;
  dot?: boolean;
}

/** The areas this viewer can use here, in order. */
function entries(p: SidebarProps): Entry[] {
  const r = p.route;
  const unseen = p.routineUnseen;
  const list: (Entry | false)[] = [
    { key: "new", to: { kind: "home" }, icon: <SquarePen size={16} />, label: t("nav.newChat"), current: r.kind === "home" },
    { key: "agents", to: { kind: "agents", id: null }, icon: <Bot size={16} />, label: t("nav.agents"), current: r.kind === "agents" },
    p.features.routines.show && {
      key: "routines",
      to: { kind: "routines", id: null },
      icon: <CalendarClock size={16} />,
      label: t("nav.routines"),
      current: r.kind === "routines",
      dot: unseen > 0,
      ...(unseen > 0
        ? { name: unseen === 1 ? t("nav.newResult", { name: t("nav.routines") }) : t("nav.newResults", { name: t("nav.routines"), n: String(unseen) }) }
        : {}),
    },
    p.features.media.show && {
      key: "media",
      to: { kind: "media", filter: null, id: null },
      icon: <Images size={16} />,
      label: t("nav.media"),
      current: r.kind === "media",
    },
    p.features.artifacts.show && {
      key: "artifacts",
      to: { kind: "artifacts", id: null },
      icon: <Package size={16} />,
      label: t("nav.artifacts"),
      current: r.kind === "artifacts",
    },
  ];
  return list.filter((e): e is Entry => !!e);
}

/** Connections: a placeholder until it is designed (no route, no code). */
function Connections({ rail }: { rail?: boolean }) {
  return (
    <button
      type="button"
      className="nav-item later"
      aria-disabled="true"
      title={t("nav.comingLater")}
      {...(rail ? { "aria-label": t("nav.connections") } : {})}
    >
      <Plug size={16} />
      {!rail && (
        <>
          <span className="grow">{t("nav.connections")}</span>
          <span className="later-tag">{t("nav.later")}</span>
        </>
      )}
    </button>
  );
}

function Section(props: {
  label: string;
  open: boolean;
  onToggle: () => void;
  action?: ReactNode;
  children: ReactNode;
}) {
  const body = useId();
  return (
    <section className="side-section">
      <div className="section-label section-row">
        <button type="button" className="section-toggle grow" aria-expanded={props.open} aria-controls={body} onClick={props.onToggle}>
          {/* Turned down by CSS while open. */}
          <ChevronRight size={12} />
          <span>{props.label}</span>
        </button>
        {props.action}
      </div>
      {props.open && (
        <div id={body} className="section-body">
          {props.children}
        </div>
      )}
    </section>
  );
}

export function Sidebar(props: SidebarProps) {
  const { me, route, onNavigate } = props;
  const [sections, setSections] = useState(readSections);
  const [menu, setMenu] = useState<MenuView | null>(null);
  const account = useRef<HTMLButtonElement>(null);
  const menuId = useId();
  const nodeById = new Map(props.nodes.map((n) => [n.node_id, n]));
  const tier = accountLine(me, me.workspace);
  const team = me.workspace && !me.workspace.personal ? me.workspace : null;

  const toggle = (id: SectionId) =>
    setSections((s) => {
      const next = { ...s, [id]: !s[id] };
      try {
        localStorage.setItem(SECTIONS_KEY, JSON.stringify(next));
      } catch {
        /* storage unavailable */
      }
      return next;
    });
  const preloadMenu = () => void loadMenu().catch(() => {});
  // Stable: the open menu listens for outside clicks with it.
  const setView = useCallback((v: MenuView | null, refocus = false) => {
    setMenu(v);
    if (refocus) account.current?.focus();
  }, []);
  const accountName = props.rail
    ? t("nav.railAccount", { name: props.displayName, tier })
    : t("nav.account", { name: props.displayName, username: me.username, tier });

  const accountButton = (
    <button
      ref={account}
      type="button"
      className={props.rail ? "icon rail-avatar" : "nav-item account"}
      aria-haspopup="menu"
      aria-expanded={menu === "menu"}
      aria-controls={menu === "menu" ? menuId : undefined}
      aria-label={accountName}
      title={accountName}
      onPointerEnter={preloadMenu}
      onFocus={preloadMenu}
      onTouchStart={preloadMenu}
      onClick={() => setView(menu === "menu" ? null : "menu")}
      onKeyDown={(e) => {
        if (e.key === "ArrowDown") {
          e.preventDefault();
          setView("menu");
        }
      }}
    >
      <span className="avatar">
        {props.displayName.slice(0, 1).toUpperCase()}
        {props.rail && team && (
          <span className="workspace-badge avatar-badge" aria-hidden="true">
            {workspaceName(team).slice(0, 1).toUpperCase()}
          </span>
        )}
      </span>
      {!props.rail && (
        <span className="account-lines grow">
          <span className="ellipsis">{props.displayName}</span>
          <span className="ellipsis muted small">{tier}</span>
        </span>
      )}
    </button>
  );
  const menuPopup = menu && (
    <Suspense fallback={null}>
      <AccountMenu
        view={menu}
        id={menuId}
        anchor={account}
        side={props.rail ? "right" : "below"}
        me={me}
        displayName={props.displayName}
        tier={tier}
        workspaces={props.workspaces}
        onSwitch={props.onSwitch}
        onNavigate={onNavigate}
        onLogout={props.onLogout}
        onView={setView}
      />
    </Suspense>
  );
  const searchButton = (
    <button
      type="button"
      className="icon"
      aria-label={t("nav.search")}
      title={t("nav.searchShortcut")}
      aria-keyshortcuts="Control+K Meta+K"
      onClick={props.onSearch}
      onPointerEnter={props.onSearchIntent}
      onFocus={props.onSearchIntent}
    >
      <Search size={18} />
    </button>
  );
  const areaLinks = entries(props).map((e) => (
    <NavLink
      key={e.key}
      to={e.to}
      current={e.current}
      onNavigate={onNavigate}
      className={props.rail ? "icon rail-item" : "nav-item"}
      label={props.rail ? (e.name ?? e.label) : e.name}
      title={props.rail ? e.label : undefined}
    >
      {e.icon}
      {!props.rail && <span className="grow">{e.label}</span>}
      {e.dot && <span className="nav-dot" aria-hidden="true" />}
    </NavLink>
  ));

  if (props.rail) {
    return (
      <aside className="sidebar rail" aria-label="Sidebar">
        <button type="button" className="icon" aria-label={t("nav.expandRail")} title={t("nav.expandRail")} onClick={props.onCollapse}>
          <Menu size={18} />
        </button>
        <nav aria-label="Main" className="rail-nav">
          {areaLinks.slice(0, 1)}
          {searchButton}
          {areaLinks.slice(1)}
          <Connections rail />
        </nav>
        <span className="spacer" />
        {accountButton}
        {menuPopup}
      </aside>
    );
  }

  const selectedChat = route.kind === "session" ? route.id : null;
  return (
    <aside className="sidebar" aria-label="Sidebar">
      <div className="sidebar-head">
        <span className="brand">{APP_TITLE}</span>
        {/* Nothing while connected; an icon only when something is wrong. */}
        {props.conn !== "live" && (
          <span className={`conn-icon conn-${props.conn}`} title={`connection: ${props.conn}`} role="status">
            {props.conn === "offline" ? <WifiOff size={14} /> : <Loader2 size={14} className="spin" />}
          </span>
        )}
        <span className="spacer" />
        {searchButton}
        <button type="button" className="icon" aria-label={t("nav.collapse")} title={t("nav.collapse")} onClick={props.onCollapse}>
          <PanelLeftClose size={18} />
        </button>
      </div>
      <nav aria-label="Main" className="sidebar-nav">
        {areaLinks}
        <Connections />
      </nav>
      <div className="sidebar-scroll">
        <Section
          label={t("nav.projects")}
          open={sections.projects}
          onToggle={() => toggle("projects")}
          action={
            props.canCreateProject && (
              <button
                type="button"
                className="icon small"
                aria-label={t("nav.newProject")}
                title={t("nav.newProject")}
                onClick={() => onNavigate({ kind: "project", id: "new" })}
              >
                <Plus size={14} />
              </button>
            )
          }
        >
          <ProjectRows projects={props.projects} selected={route.kind === "project" ? route.id : null} onNavigate={onNavigate} />
        </Section>
        <Section label={t("nav.recents")} open={sections.recent} onToggle={() => toggle("recent")}>
          {props.sessions.length === 0 && <div className="muted pad small">{t("nav.noChats")}</div>}
          {props.sessions.map((s) => (
            <NavLink
              key={s.session_id}
              to={{ kind: "session", id: s.session_id }}
              current={s.session_id === selectedChat}
              onNavigate={onNavigate}
              className="session-item"
              title={s.title}
            >
              {s.running && <span className="dot running" title="running" />}
              <span className="ellipsis grow">{s.title || t("session.untitled")}</span>
              <ChatProjectMark id={s.project_id} projects={props.projects} />
              {s.profile === "agentic" && <NodeBadge node={nodeById.get(s.node_id)} nodeId={s.node_id} />}
            </NavLink>
          ))}
        </Section>
      </div>
      <div className="sidebar-foot">{accountButton}</div>
      {menuPopup}
    </aside>
  );
}

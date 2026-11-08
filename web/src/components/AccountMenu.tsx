// The account menu (docs/sidebar-search.md), a lazy chunk the sidebar loads
// on intent: the account's header row (opens Workspaces), Profile, Settings
// and Log out. Workspaces is a submenu beside it, or drilled in place on a
// narrow screen or a touch pointer. A new workspace is named in a small
// dialog: a text field may not live inside a menu.
import { Check, ChevronLeft, ChevronRight, Plus } from "lucide-react";
import { type FormEvent, type KeyboardEvent, type ReactNode, type RefObject, useEffect, useRef, useState } from "react";
import { api } from "../api";
import { t } from "../lib/i18n";
import type { Route } from "../lib/routes";
import { workspaceName } from "../lib/workspaces";
import { type Side, useAnchored } from "../md/anchored";
import type { Me, WorkspaceList, WorkspaceSummary } from "../types";
import { NavLink } from "./NavLink";

/** What shows: the menu, or the dialog naming a new workspace. */
export type MenuView = "menu" | "new";

export interface AccountMenuProps {
  view: MenuView;
  /** The menu's element id (the button's `aria-controls`). */
  id: string;
  anchor: RefObject<HTMLButtonElement | null>;
  side: Side;
  me: Me;
  displayName: string;
  tier: string;
  workspaces: WorkspaceList | null;
  onSwitch: (id: string) => Promise<void>;
  onNavigate: (r: Route) => void;
  onLogout: () => void;
  /** Show another view, or close (null); `refocus` returns focus to the button. */
  onView: (v: MenuView | null, refocus?: boolean) => void;
}

// The menu's own words live in its chunk, not in the first load's catalog.
const TEXT = { menu: "Account", profile: "Profile", settings: "Settings", workspaces: "Workspaces", back: "Back" };

/** Hover opens the submenu after this long (fine pointers only). */
const HOVER_MS = 120;

const message = (x: unknown) => (x instanceof Error ? x.message : String(x));

/** Narrow screens and touch pointers drill into the submenu in place. */
function drills(): boolean {
  const m = (q: string) => window.matchMedia?.(q).matches ?? false;
  return m("(max-width: 760px)") || m("(pointer: coarse)");
}

/** Personal first, then team workspaces A–Z. */
function ordered(list: WorkspaceSummary[]): WorkspaceSummary[] {
  const teams = list.filter((w) => !w.personal).sort((a, b) => a.name.localeCompare(b.name, undefined, { sensitivity: "base" }));
  return [...list.filter((w) => w.personal), ...teams];
}

/** The items of a menu element, in order. */
const items = (el: HTMLElement | null) => [...(el?.querySelectorAll<HTMLElement>('[role^="menuitem"]') ?? [])];

/** ↑/↓/Home/End move focus through `el`'s items, wrapping. */
function moveFocus(e: KeyboardEvent, el: HTMLElement | null): boolean {
  const list = items(el);
  const i = list.indexOf(document.activeElement as HTMLElement);
  const to =
    e.key === "ArrowDown" ? (i + 1) % list.length : e.key === "ArrowUp" ? (i - 1 + list.length) % list.length : e.key === "Home" ? 0 : e.key === "End" ? list.length - 1 : -1;
  if (to < 0 || list.length === 0) return false;
  e.preventDefault();
  list[to]!.focus();
  return true;
}

export default function AccountMenu(props: AccountMenuProps) {
  return props.view === "new" ? <NewWorkspace {...props} /> : <Menu {...props} />;
}

function Menu(props: AccountMenuProps) {
  const { me, onView } = props;
  const box = useRef<HTMLDivElement>(null);
  const sub = useRef<HTMLDivElement>(null);
  const head = useRef<HTMLDivElement>(null);
  const hover = useRef<ReturnType<typeof setTimeout> | null>(null);
  const [drill] = useState(drills);
  // The submenu; `focus` moves focus into it (keys) rather than only showing it (hover).
  const [open, setOpen] = useState<{ focus: boolean } | null>(null);
  const [err, setErr] = useState<string | null>(null);
  useAnchored(box, props.anchor, props.side);

  // Opening focuses the first item (APG menu button).
  useEffect(() => items(box.current)[0]?.focus(), []);
  useEffect(() => {
    const onDown = (e: MouseEvent) => {
      const inside = [box, sub, props.anchor].some((r) => r.current?.contains(e.target as Node));
      if (!inside) onView(null);
    };
    document.addEventListener("mousedown", onDown);
    return () => {
      document.removeEventListener("mousedown", onDown);
      if (hover.current) clearTimeout(hover.current);
    };
  }, [onView, props.anchor]);
  // Into the submenu on its checked workspace; back out onto the header
  // row once it is rendered again (drilled) or the flyout is gone.
  const backToHead = useRef(false);
  useEffect(() => {
    if (open?.focus) {
      const el = drill ? box.current : sub.current;
      (el?.querySelector<HTMLElement>('[aria-checked="true"]') ?? items(el)[drill ? 1 : 0])?.focus();
    } else if (!open && backToHead.current) {
      backToHead.current = false;
      head.current?.focus();
    }
  }, [open, drill]);

  const back = () => {
    backToHead.current = true;
    setOpen(null);
    setErr(null);
  };
  const close = (refocus: boolean) => onView(null, refocus);
  const go = (r: Route) => {
    close(false);
    props.onNavigate(r);
  };
  const keys = (e: KeyboardEvent, inSub: boolean) => {
    if (e.key === "Tab" || (e.key === "Escape" && !inSub)) {
      e.preventDefault();
      close(true);
    } else if (inSub && (e.key === "Escape" || e.key === "ArrowLeft")) {
      e.preventDefault();
      back();
    } else if (!inSub && e.key === "ArrowRight" && document.activeElement === head.current) {
      e.preventDefault();
      setOpen({ focus: true });
    } else {
      moveFocus(e, inSub && !drill ? sub.current : box.current);
    }
  };
  const pick = (w: WorkspaceSummary) => {
    setErr(null);
    if (w.workspace_id === me.workspace?.workspace_id) return close(true);
    props.onSwitch(w.workspace_id).catch((x) => setErr(message(x)));
  };
  // Hovering the header row opens the flyout; another item closes it.
  const hoverHead = (on: boolean) => {
    if (hover.current) clearTimeout(hover.current);
    if (on && !drill && !open) hover.current = setTimeout(() => setOpen({ focus: false }), HOVER_MS);
  };
  const leaveSub = () => {
    if (!drill && open && !open.focus) setOpen(null);
  };

  const list = ordered(props.workspaces?.workspaces ?? (me.workspace ? [me.workspace] : []));
  const workspaceItems: ReactNode = (
    <>
      {drill && (
        <button type="button" role="menuitem" className="menu-item" onClick={back}>
          <ChevronLeft size={14} /> {TEXT.back}
        </button>
      )}
      {list.map((w) => {
        const on = w.workspace_id === me.workspace?.workspace_id;
        return (
          <button
            key={w.workspace_id}
            type="button"
            role="menuitemradio"
            aria-checked={on}
            className="menu-item"
            onClick={() => pick(w)}
          >
            {w.personal ? (
              <span className="avatar small-avatar" aria-hidden="true">
                {props.displayName.slice(0, 1).toUpperCase()}
              </span>
            ) : (
              <span className="workspace-badge" aria-hidden="true">
                {workspaceName(w).slice(0, 1).toUpperCase()}
              </span>
            )}
            <span className="ellipsis grow">{w.personal ? props.displayName : w.name}</span>
            <span className="muted small">{w.personal ? t("workspace.personal") : t(`role.${w.role}`)}</span>
            <span className="menu-check">{on && <Check size={14} />}</span>
          </button>
        );
      })}
      {err && (
        <div className="menu-error error-text small" role="alert">
          {err}
        </div>
      )}
      <div role="separator" className="menu-sep" />
      {props.workspaces?.can_create && (
        <button type="button" role="menuitem" className="menu-item" onClick={() => onView("new")}>
          <Plus size={14} /> {t("workspace.new")}
        </button>
      )}
      <NavLink to={{ kind: "settings", section: "workspace" }} onNavigate={go} role="menuitem" className="menu-item">
        {t("workspace.settings")}
      </NavLink>
    </>
  );

  const header = (
    <div
      ref={head}
      role="menuitem"
      tabIndex={-1}
      aria-haspopup="menu"
      aria-expanded={!!open}
      aria-label={`${t("nav.account", { name: props.displayName, username: me.username, tier: props.tier })} — ${TEXT.workspaces}`}
      className="menu-item menu-head"
      onClick={() => setOpen(open ? null : { focus: true })}
      onKeyDown={(e) => {
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          setOpen({ focus: true });
        }
      }}
      onPointerEnter={() => hoverHead(true)}
      onPointerLeave={() => hoverHead(false)}
    >
      <span className="avatar" aria-hidden="true">
        {props.displayName.slice(0, 1).toUpperCase()}
      </span>
      <span className="account-lines grow">
        <span className="ellipsis">{props.displayName}</span>
        <span className="ellipsis muted small">{props.tier}</span>
      </span>
      <ChevronRight size={14} aria-hidden="true" />
    </div>
  );

  return (
    <>
      <div
        ref={box}
        id={props.id}
        className="account-menu"
        popover="manual"
        role="menu"
        aria-label={drill && open ? TEXT.workspaces : TEXT.menu}
        onKeyDown={(e) => keys(e, drill && !!open)}
      >
        {drill && open ? (
          workspaceItems
        ) : (
          <>
            {header}
            <div role="separator" className="menu-sep" />
            <div onPointerEnter={leaveSub}>
              <NavLink to={{ kind: "settings", section: "account", tab: "profile" }} onNavigate={go} role="menuitem" className="menu-item">
                {TEXT.profile}
              </NavLink>
              <NavLink to={{ kind: "settings", section: "account", tab: "preferences" }} onNavigate={go} role="menuitem" className="menu-item">
                {TEXT.settings}
              </NavLink>
              <div role="separator" className="menu-sep" />
              <button type="button" role="menuitem" className="menu-item" onClick={props.onLogout}>
                {t("nav.logout")}
              </button>
            </div>
          </>
        )}
      </div>
      {!drill && open && (
        <Submenu box={sub} anchor={head} onKeyDown={(e) => keys(e, true)}>
          {workspaceItems}
        </Submenu>
      )}
    </>
  );
}

/** The Workspaces flyout, beside the header row. */
function Submenu(props: { box: RefObject<HTMLDivElement | null>; anchor: RefObject<HTMLElement | null>; onKeyDown: (e: KeyboardEvent) => void; children: ReactNode }) {
  useAnchored(props.box, props.anchor, "right");
  return (
    <div ref={props.box} className="account-menu" popover="manual" role="menu" aria-label={TEXT.workspaces} onKeyDown={props.onKeyDown}>
      {props.children}
    </div>
  );
}

/** Names a new workspace, creates it and switches to it. */
function NewWorkspace(props: AccountMenuProps) {
  const dialog = useRef<HTMLDialogElement>(null);
  const [name, setName] = useState("");
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  // Leaving the DOM takes it out of the top layer; closing it (Cancel, Esc)
  // is what ends this view.
  useEffect(() => dialog.current?.showModal?.(), []);
  const create = async (e: FormEvent) => {
    e.preventDefault();
    const n = name.trim();
    if (!n) return;
    setBusy(true);
    setErr(null);
    try {
      const w = await api.createWorkspace(n);
      await props.onSwitch(w.workspace_id);
      props.onView(null);
    } catch (x) {
      setErr(message(x));
      setBusy(false);
    }
  };
  return (
    <dialog ref={dialog} className="workspace-dialog" aria-label={t("workspace.new")} onClose={() => props.onView(null, true)}>
      <form className="workspace-new" onSubmit={(e) => void create(e)}>
        <input
          autoFocus
          maxLength={64}
          placeholder={t("workspace.newName")}
          aria-label={t("workspace.newName")}
          value={name}
          onChange={(e) => setName(e.target.value)}
        />
        {err && (
          <div className="error-text small" role="alert">
            {err}
          </div>
        )}
        <div className="row">
          <button type="submit" className="primary" disabled={busy || !name.trim()}>
            {t("workspace.create")}
          </button>
          <button type="button" onClick={() => dialog.current?.close()}>
            {t("workspace.cancel")}
          </button>
        </div>
      </form>
    </dialog>
  );
}

import { Menu, SquarePen } from "lucide-react";
import { APP_TITLE } from "../lib/brand";
import { lazy, Suspense, useCallback, useEffect, useMemo, useRef, useState, useSyncExternalStore } from "react";
import { api, HttpError } from "../api";
import { Live } from "../live";
import { t } from "../lib/i18n";
import { type ClassifierState, decide, emptyState, parseState, record, serialize } from "../lib/classifier";
import { agenticBlockReason, type Mode } from "../lib/modes";
import { NODES_HASH, parseRoute, type Route, routeHash } from "../lib/routes";
import { afterChange, followRoute, keepsRoute, workspaceName } from "../lib/workspaces";
import { buildSearchUrl } from "../lib/searchUrl";
import { titleFrom } from "../lib/title";
import { applyAppearance, writeBackdropMirror } from "../lib/appearance";
import { newlyWaiting, notifyEnabled } from "../lib/notify";
import type {
  Agent,
  AttentionItem,
  Me,
  ModelInfo,
  NodeAttention,
  NodeInfo,
  Notice,
  Prefs,
  SessionRow,
  WorkspaceList,
  WorkspaceSummary,
} from "../types";
import type { Submission } from "./Composer";
import { Home } from "./Home";
import { Sidebar } from "./Sidebar";

// Everything but the home screen loads on demand (new-tab speed budget).
const Conversation = lazy(() => import("./Conversation"));
const Settings = lazy(() => import("./Settings"));
const Onboarding = lazy(() => import("./Onboarding"));

const DEFAULT_SEARCH = "https://duckduckgo.com/?q={q}";
/** A home visit's background seed (bg/rng.ts would bring the whole PRNG into the first load). */
const newSeed = () => crypto.getRandomValues(new Uint32Array(1))[0]!;
const COLLAPSED_KEY = "harness.sidebarCollapsed";
/** Models can drop out of a provider's listing at any time (the server re-lists daily). */
const NOTICES_MS = 5 * 60_000;

function go(r: Route) {
  location.hash = routeHash(r);
}

function useRoute(): Route {
  const [r, setR] = useState(() => parseRoute(location.hash));
  useEffect(() => {
    const on = () => setR(parseRoute(location.hash));
    window.addEventListener("hashchange", on);
    return () => window.removeEventListener("hashchange", on);
  }, []);
  return r;
}

export function Shell(props: {
  me: Me;
  onLogout: () => void;
  /** Reloads `me`, showing `keep` (refreshed) when given, else the server's
   * active workspace; another workspace remounts Shell (App keys it). */
  onMeChanged: (keep?: WorkspaceSummary) => Promise<void>;
  /** The workspace the user was taken out of, to say so once; null when nothing to say. */
  gone: string | null;
  onGone: (name: string | null) => void;
}) {
  const { me, onLogout, onMeChanged, onGone } = props;
  // Fixed for this Shell's life (App remounts it on a switch); every list and
  // new chat names it, so nothing depends on the server's idea of "active".
  const workspaceId = me.workspace?.workspace_id ?? null;
  const [live] = useState(() => new Live());
  // null until the first load, so a chat the route opens is not taken for
  // one from another workspace before this one's chats are known.
  const [loadedSessions, setSessions] = useState<SessionRow[] | null>(null);
  const sessions = useMemo(() => loadedSessions ?? [], [loadedSessions]);
  const [workspaces, setWorkspaces] = useState<WorkspaceList | null>(null);
  const [nodes, setNodes] = useState<NodeInfo[]>([]);
  const [agents, setAgents] = useState<Agent[]>([]);
  // The default agent where this Shell lists: the workspace's, else the server's.
  const [scopeDefaultAgent, setScopeDefaultAgent] = useState("general");
  const [models, setModels] = useState<ModelInfo[]>([]);
  const [prefs, setPrefs] = useState<Prefs | null>(null);
  const [memoryVersion, setMemoryVersion] = useState(0);
  const [toReview, setToReview] = useState(0);
  const [voice, setVoice] = useState(false);
  const [attention, setAttention] = useState<AttentionItem[]>([]);
  const [nodeAttention, setNodeAttention] = useState<NodeAttention[]>([]);
  const [notices, setNotices] = useState<Notice[]>([]);
  const waiting = useRef<string[] | null>(null);
  const [collapsed, setCollapsed] = useState(() => {
    try {
      return localStorage.getItem(COLLAPSED_KEY) === "1";
    } catch {
      return false;
    }
  });
  const [drawer, setDrawer] = useState(false);
  const route = useRoute();
  // Each visit to the home screen re-rolls its background: arriving there,
  // or New chat while already there (Home stays mounted then).
  const [home, setHome] = useState(() => ({ kind: route.kind, seed: newSeed() }));
  if (home.kind !== route.kind) setHome({ kind: route.kind, seed: route.kind === "home" ? newSeed() : home.seed });
  const classifier = useRef<ClassifierState>(emptyState());
  const saveTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  const unauthorized = useCallback(
    (e: unknown) => {
      if (e instanceof HttpError && e.status === 401) onLogout();
    },
    [onLogout],
  );
  const refreshAttention = useCallback(() => {
    api
      .attention()
      .then((r) => {
        const nodes = r.nodes ?? [];
        const ids = [...r.items.map((a) => a.session_id), ...nodes.map((n) => `node:${n.setup_id}`)];
        // Notify about chats that just started waiting (not on first load).
        if (waiting.current && notifyEnabled()) {
          for (const id of newlyWaiting(waiting.current, ids)) {
            if (id.startsWith("node:")) {
              const n = nodes.find((x) => `node:${x.setup_id}` === id)!;
              const note = new Notification(`Server ${n.host}`, { body: n.problems.map((p) => p.detail).join("; "), tag: id });
              note.onclick = () => {
                window.focus();
                location.hash = NODES_HASH;
                note.close();
              };
              continue;
            }
            const a = r.items.find((x) => x.session_id === id)!;
            const viewing = location.hash === `#/s/${id}` && document.visibilityState === "visible";
            if (viewing) continue;
            const n = new Notification(a.title || APP_TITLE, {
              body: a.reason === "approval" ? "Needs your approval" : "Asked you a question",
              tag: id,
            });
            n.onclick = () => {
              window.focus();
              location.hash = `#/s/${id}`;
              n.close();
            };
          }
        }
        waiting.current = ids;
        setAttention(r.items);
        setNodeAttention(nodes);
      })
      .catch(() => {});
  }, []);
  const refreshSessions = useCallback(() => {
    api
      .sessions(workspaceId)
      .then((r) => setSessions(r.sessions))
      .catch(unauthorized);
    refreshAttention();
  }, [workspaceId, unauthorized, refreshAttention]);
  // Models, agents and notices follow the workspace's defaults and chains.
  const scope = useMemo(() => (workspaceId ? { workspace: workspaceId } : null), [workspaceId]);
  const refreshNotices = useCallback(() => {
    api
      .notices(scope)
      .then((r) => setNotices(r.notices))
      .catch(() => {});
  }, [scope]);
  // Agents too: their default models change with the provider settings, and
  // so do the notices about models gone from a provider.
  const refreshModels = useCallback(() => {
    api
      .models(scope)
      .then((r) => setModels(r.models))
      .catch(unauthorized);
    refreshNotices();
    api
      .agents(scope)
      .then((r) => {
        setAgents(r.agents);
        setScopeDefaultAgent(r.default);
      })
      .catch(unauthorized);
  }, [scope, unauthorized, refreshNotices]);
  const refreshNodes = useCallback(() => {
    api.nodes().then((r) => setNodes(r.nodes)).catch(unauthorized);
  }, [unauthorized]);
  // The latest `me`, for handlers that outlive a render.
  const meRef = useRef(me);
  useEffect(() => {
    meRef.current = me;
  });
  // The workspace the user just left or deleted here: its going is no news.
  const left = useRef<string | null>(null);
  // A workspace, member, role or policy changed somewhere (or the socket came
  // back): reload the list, who the user is here (their role), what the
  // workspace's settings decide, and the chats (a role decides which ones
  // are read-only). The tab stays in its workspace while the user is in it
  // (lib/workspaces.ts says why); taken out, it falls back to the server's.
  const workspacesChanged = useCallback(() => {
    refreshModels();
    refreshSessions();
    const here = meRef.current.workspace;
    if (!here) return;
    api
      .workspaces()
      .then((list) => {
        setWorkspaces(list);
        const next = afterChange(here, list.workspaces, left.current);
        if ("keep" in next) return onMeChanged(next.keep);
        // Said once the fallback shows, since the line names it.
        return onMeChanged().then(() => next.gone && onGone(next.gone));
      })
      .catch(unauthorized);
  }, [refreshModels, refreshSessions, unauthorized, onGone, onMeChanged]);
  // Left or deleted from its settings: the same reload, without the line
  // saying the user is no longer in it.
  const workspaceLeft = useCallback(() => {
    left.current = workspaceId;
    workspacesChanged();
  }, [workspaceId, workspacesChanged]);

  useEffect(() => {
    live.onSessionsChanged = refreshSessions;
    live.onNodesChanged = refreshNodes;
    live.onMemoryChanged = () => setMemoryVersion((v) => v + 1);
    live.onWorkspacesChanged = workspacesChanged;
    refreshSessions();
    refreshNodes();
    refreshModels();
    // The switcher's list; the API token has no workspaces.
    if (meRef.current.workspace) api.workspaces().then(setWorkspaces).catch(unauthorized);
    api.settings().then(setPrefs).catch(unauthorized);
    api
      .features()
      .then((f) => setVoice(f.voice))
      .catch(() => {});
    api
      .classifier()
      .then((v) => {
        classifier.current = parseState(v);
      })
      .catch(() => {});
  }, [live, refreshSessions, refreshNodes, refreshModels, workspacesChanged, unauthorized]);
  // Closed only when this Shell goes (a switch remounts it): a closed Live
  // never reconnects, so it must not follow the data effect's dependencies.
  useEffect(() => () => live.close(), [live]);

  // With the agents, whose red marks follow the same listings.
  useEffect(() => {
    const id = setInterval(refreshModels, NOTICES_MS);
    return () => clearInterval(id);
  }, [refreshModels]);

  useEffect(() => {
    api
      .memories()
      .then((r) => setToReview(r.memories.filter((f) => f.status === "proposed").length))
      .catch(() => setToReview(0));
  }, [memoryVersion]);

  // Tab title shows how many chats wait for the user.
  useEffect(() => {
    const n = attention.length + nodeAttention.length;
    document.title = n ? `(${n}) ${APP_TITLE}` : APP_TITLE;
  }, [attention.length, nodeAttention.length]);

  // Looking at a chat that asked a question counts as seeing it.
  const viewing = route.kind === "session" ? route.id : null;
  useEffect(() => {
    const a = attention.find((x) => x.session_id === viewing && x.reason === "question");
    if (!a) return;
    const mark = () => {
      if (document.visibilityState === "visible") void api.markSeen(a.session_id, a.seq).catch(() => {});
    };
    mark();
    document.addEventListener("visibilitychange", mark);
    return () => document.removeEventListener("visibilitychange", mark);
  }, [viewing, attention]);

  // Theme and accent from preferences, once they are loaded: until then
  // public/theme.js has applied the saved ones, and clearing them would
  // flash the defaults.
  const loaded = prefs !== null;
  useEffect(() => {
    if (loaded) applyAppearance(prefs?.theme, prefs?.accent);
  }, [loaded, prefs?.theme, prefs?.accent]);
  useEffect(() => {
    if (loaded) writeBackdropMirror(prefs?.backdrop);
  }, [loaded, prefs?.backdrop]);

  useSyncExternalStore(
    (l) => live.subscribe(l),
    () => live.version,
  );

  const toggleCollapsed = () => {
    setCollapsed((c) => {
      try {
        localStorage.setItem(COLLAPSED_KEY, c ? "0" : "1");
      } catch {
        /* storage unavailable */
      }
      return !c;
    });
  };

  const logout = async () => {
    await api.logout().catch(() => {});
    onLogout();
  };

  const listed = useMemo(() => sessions.filter((x) => !x.incognito && !x.parent_id), [sessions]);
  // The Ask wall infers topics from the titles the sidebar lists (untitled
  // chats say nothing), and keeps every chat's title off the wall, the
  // incognito and sub-agent ones the sidebar hides included.
  const history = useMemo(
    () => ({
      titles: listed.filter((x) => x.title && x.title !== t("session.untitled")).map((x) => ({ title: x.title, updated_ms: x.updated_ms })),
      banned: sessions.map((x) => x.title).filter(Boolean),
    }),
    [listed, sessions],
  );
  const gutter = prefs?.backdrop_chats && prefs.backdrop !== "off" ? history : null;
  const workers = nodes.filter((n) => n.connected && n.profile === "agentic");
  const agenticBlocked = agenticBlockReason(me.can_agentic, me.workspace, workers.length);
  const defaultAgent = prefs?.default_agent ?? scopeDefaultAgent;
  const selected = route.kind === "session" ? route.id : null;

  // Makes another workspace the active one; App then remounts Shell there.
  // Staying on the route is for a chat that belongs there; else home. The
  // tab lands in the workspace it asked for, not in whatever /api/me says by
  // then: another tab switching at the same moment must not bounce it.
  const switchTo = useCallback(
    async (id: string, keepRoute: boolean) => {
      await api.switchWorkspace(id);
      const known = (l: WorkspaceList | null) => l?.workspaces.find((w) => w.workspace_id === id);
      // One just created is not in this tab's list yet.
      const target = known(workspaces) ?? known(await api.workspaces());
      onGone(null);
      if (!keepRoute) go({ kind: "home" });
      await onMeChanged(target);
    },
    [workspaces, onGone, onMeChanged],
  );

  // A chat opened from another workspace (a notification, Attention, a
  // sub-task's parent link, back/forward) shows in its own: look it up, then
  // switch there, staying on it (lib/workspaces.ts decides).
  const [found, setFound] = useState<{ id: string; row: SessionRow | null } | null>(null);
  const lookup = found && found.id === selected ? found : null;
  const follow = followRoute({
    id: selected,
    active: workspaceId,
    listed: loadedSessions ? loadedSessions.some((s) => s.session_id === selected) : null,
    found: lookup ? (lookup.row?.workspace_id ?? null) : undefined,
    memberOf: workspaces ? workspaces.workspaces.map((w) => w.workspace_id) : null,
  });
  const followKey = follow.do === "lookup" ? `lookup:${follow.id}` : follow.do === "switch" ? `switch:${follow.to}` : "";
  useEffect(() => {
    if (follow.do === "lookup") {
      const id = follow.id;
      api
        .session(id)
        .then((row) => setFound({ id, row }))
        .catch(() => setFound({ id, row: null }));
    } else if (follow.do === "switch") {
      switchTo(follow.to, true).catch(unauthorized);
    }
    // Once per step: the key says everything the step depends on.
  }, [followKey]);
  // The open chat's row: from this workspace's list, else from the lookup.
  const openRow = selected ? (sessions.find((s) => s.session_id === selected) ?? lookup?.row ?? null) : null;

  const submit = async (s: Submission) => {
    if (s.mode === "search") {
      const q = s.search ?? { url: prefs?.search_url ?? DEFAULT_SEARCH, query: s.text };
      location.href = buildSearchUrl(q.url, q.query);
      return;
    }
    const agentic = s.mode === "agentic";
    const { session_id } = await api.createSession({
      profile: agentic ? "agentic" : "chat",
      title: titleFrom(s.text),
      agent_id: s.agentId,
      model: s.model || undefined,
      node_id: agentic ? s.nodeId : undefined,
      cwd: agentic ? s.cwd : undefined,
      ...(s.incognito ? { incognito: true } : {}),
      ...(workspaceId ? { workspace_id: workspaceId } : {}),
    });
    refreshSessions();
    go({ kind: "session", id: session_id });
    await api.prompt(session_id, s.text, {
      ...(s.effort ? { effort: s.effort } : {}),
      ...(s.attachments.length ? { attachments: s.attachments } : {}),
      ...(s.compareWith ? { compare: [s.model, s.compareWith] } : {}),
    });
  };

  const agenticAllowed = agenticBlocked === null;
  const predict = useCallback(
    (text: string) => decide(classifier.current, text, agenticAllowed).mode,
    [agenticAllowed],
  );
  const learn = useCallback(
    (text: string, mode: Mode) => {
      record(classifier.current, text, mode, agenticAllowed);
      // Only weights and accuracy counters are saved, never the text.
      if (saveTimer.current) clearTimeout(saveTimer.current);
      saveTimer.current = setTimeout(() => {
        void api.saveClassifier(serialize(classifier.current)).catch(() => {});
      }, 1000);
    },
    [agenticAllowed],
  );

  const navigate = (r: Route) => {
    setDrawer(false);
    if (r.kind === "home" && route.kind === "home") setHome({ kind: "home", seed: newSeed() });
    go(r);
  };

  if (prefs && !prefs.onboarded) {
    return (
      <Suspense fallback={<div className="splash" />}>
        <Onboarding
          me={me}
          prefs={prefs}
          onDone={(p) => {
            setPrefs(p);
            refreshModels();
          }}
        />
      </Suspense>
    );
  }

  return (
    <div className={`shell ${collapsed ? "collapsed" : ""} ${drawer ? "drawer-open" : ""}`}>
      {!collapsed && (
        <Sidebar
          me={me}
          sessions={listed}
          nodes={nodes}
          selected={selected}
          conn={live.state}
          workspaces={workspaces}
          onSwitch={(id) => switchTo(id, keepsRoute(openRow, id))}
          onWorkspaceSettings={() => navigate({ kind: "settings", section: "workspace" })}
          onSelect={(id) => navigate({ kind: "session", id })}
          onNew={() => navigate({ kind: "home" })}
          onSettings={() => navigate({ kind: "settings" })}
          onLogout={logout}
          onCollapse={() => (drawer ? setDrawer(false) : toggleCollapsed())}
        />
      )}
      {drawer && <div className="scrim" onClick={() => setDrawer(false)} />}
      <main className="main">
        <div className="topbar">
          <button
            className="icon"
            aria-label={t("nav.expand")}
            onClick={() => (collapsed ? toggleCollapsed() : setDrawer(true))}
          >
            <Menu size={18} />
          </button>
          {collapsed && (
            <button className="icon" aria-label={t("nav.newChat")} onClick={() => navigate({ kind: "home" })}>
              <SquarePen size={18} />
            </button>
          )}
        </div>
        {props.gone && (
          <div className="workspace-gone small" role="status">
            <span>{t("workspace.gone", { name: props.gone, here: me.workspace ? workspaceName(me.workspace) : "" })}</span>
            <button type="button" className="link" onClick={() => onGone(null)}>
              {t("workspace.dismiss")}
            </button>
          </div>
        )}
        <Suspense fallback={<div className="muted pad">Loading…</div>}>
          {route.kind === "session" ? (
            <Conversation
              key={route.id}
              live={live}
              sessionId={route.id}
              row={openRow}
              nodes={nodes}
              agents={agents}
              models={models}
              voice={voice}
              gutter={gutter}
            />
          ) : route.kind === "settings" ? (
            <Settings
              me={me}
              agents={agents}
              models={models}
              defaultAgent={scopeDefaultAgent}
              prefs={prefs}
              onPrefs={(p) => {
                setPrefs(p);
                // Whether a notice is "your default" follows the saved prefs.
                refreshNotices();
              }}
              onProviderChanged={refreshModels}
              onAgentsChanged={refreshModels}
              onWorkspaceChanged={workspacesChanged}
              onWorkspaceLeft={workspaceLeft}
              route={route}
              memoryVersion={memoryVersion}
              nodes={nodes}
            />
          ) : (
            <Home
              username={prefs?.display_name || me.username}
              timeZone={prefs?.timezone}
              agents={agents}
              defaultAgent={defaultAgent}
              models={models}
              workers={workers}
              prefs={prefs}
              agenticBlocked={agenticBlocked}
              workspace={workspaceId}
              onSubmit={submit}
              predict={predict}
              onLearn={learn}
              onOpen={(id) => navigate({ kind: "session", id })}
              toReview={toReview}
              voice={voice}
              attention={attention}
              nodeAttention={nodeAttention}
              notices={notices}
              seed={home.seed}
              history={history}
            />
          )}
        </Suspense>
      </main>
    </div>
  );
}

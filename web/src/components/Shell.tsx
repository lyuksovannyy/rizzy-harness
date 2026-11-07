import { Menu, SquarePen } from "lucide-react";
import { APP_TITLE } from "../lib/brand";
import { lazy, Suspense, useCallback, useEffect, useMemo, useRef, useState, useSyncExternalStore } from "react";
import { api, HttpError } from "../api";
import { Live } from "../live";
import { t } from "../lib/i18n";
import { type ClassifierState, decide, emptyState, parseState, record, serialize } from "../lib/classifier";
import { agenticBlockReason, type Mode } from "../lib/modes";
import { buildSearchUrl } from "../lib/searchUrl";
import { titleFrom } from "../lib/title";
import { applyAppearance, writeBackdropMirror } from "../lib/appearance";
import { newlyWaiting, notifyEnabled } from "../lib/notify";
import type { Agent, AttentionItem, Me, ModelInfo, NodeAttention, NodeInfo, Prefs, SessionRow } from "../types";
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

type Route =
  | { kind: "home" }
  | { kind: "session"; id: string }
  | { kind: "settings"; tab?: "memory" | "nodes" };

function parseRoute(): Route {
  const h = location.hash;
  const m = h.match(/^#\/s\/([0-9a-f-]{36})$/);
  if (m) return { kind: "session", id: m[1]! };
  if (h === "#/settings") return { kind: "settings" };
  if (h === "#/settings/memory") return { kind: "settings", tab: "memory" };
  if (h === "#/settings/nodes") return { kind: "settings", tab: "nodes" };
  return { kind: "home" };
}

function go(r: Route) {
  location.hash = r.kind === "session" ? `#/s/${r.id}` : r.kind === "settings" ? "#/settings" : "#/";
}

function useRoute(): Route {
  const [r, setR] = useState(parseRoute);
  useEffect(() => {
    const on = () => setR(parseRoute());
    window.addEventListener("hashchange", on);
    return () => window.removeEventListener("hashchange", on);
  }, []);
  return r;
}

export function Shell({ me, onLogout }: { me: Me; onLogout: () => void }) {
  const [live] = useState(() => new Live());
  const [sessions, setSessions] = useState<SessionRow[]>([]);
  const [nodes, setNodes] = useState<NodeInfo[]>([]);
  const [agents, setAgents] = useState<Agent[]>([]);
  const [serverDefaultAgent, setServerDefaultAgent] = useState("general");
  const [models, setModels] = useState<ModelInfo[]>([]);
  const [prefs, setPrefs] = useState<Prefs | null>(null);
  const [memoryVersion, setMemoryVersion] = useState(0);
  const [toReview, setToReview] = useState(0);
  const [voice, setVoice] = useState(false);
  const [attention, setAttention] = useState<AttentionItem[]>([]);
  const [nodeAttention, setNodeAttention] = useState<NodeAttention[]>([]);
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
                location.hash = "#/settings/nodes";
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
    api.sessions().then((r) => setSessions(r.sessions)).catch(unauthorized);
    refreshAttention();
  }, [unauthorized, refreshAttention]);
  // Agents too: their default models change with the provider settings.
  const refreshModels = useCallback(() => {
    api.models().then((r) => setModels(r.models)).catch(unauthorized);
    api
      .agents()
      .then((r) => {
        setAgents(r.agents);
        setServerDefaultAgent(r.default);
      })
      .catch(unauthorized);
  }, [unauthorized]);
  const refreshNodes = useCallback(() => {
    api.nodes().then((r) => setNodes(r.nodes)).catch(unauthorized);
  }, [unauthorized]);

  useEffect(() => {
    live.onSessionsChanged = refreshSessions;
    live.onNodesChanged = refreshNodes;
    live.onMemoryChanged = () => setMemoryVersion((v) => v + 1);
    refreshSessions();
    refreshNodes();
    refreshModels();
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
    return () => live.close();
  }, [live, refreshSessions, refreshNodes, refreshModels, unauthorized]);

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
  const agenticBlocked = agenticBlockReason(me.can_agentic, workers.length);
  const defaultAgent = prefs?.default_agent ?? serverDefaultAgent;
  const selected = route.kind === "session" ? route.id : null;

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
        <Suspense fallback={<div className="muted pad">Loading…</div>}>
          {route.kind === "session" ? (
            <Conversation
              key={route.id}
              live={live}
              sessionId={route.id}
              row={sessions.find((s) => s.session_id === route.id) ?? null}
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
              prefs={prefs}
              onPrefs={setPrefs}
              onProviderChanged={refreshModels}
              initialTab={route.tab}
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
              onSubmit={submit}
              predict={predict}
              onLearn={learn}
              onOpen={(id) => navigate({ kind: "session", id })}
              toReview={toReview}
              voice={voice}
              attention={attention}
              nodeAttention={nodeAttention}
              seed={home.seed}
              history={history}
            />
          )}
        </Suspense>
      </main>
    </div>
  );
}

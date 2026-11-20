// One WebSocket for everything. Each subscribed session keeps a cursor (its
// folded head_seq); on reconnect we resubscribe from that cursor, and the
// server replays exactly what we missed. Duplicates are dropped by seq, and a
// gap (which the server should never produce) triggers a resubscribe.
import { loginKey } from "./api";
import { emptyView } from "./emptyView";
import type { RawEvent, SessionView } from "./types";

type Fold = typeof import("./fold");

export type ConnState = "connecting" | "live" | "offline";

type Listener = () => void;

/** The areas whose lists change without content on the wire (`<topic>_changed`). */
export type LiveTopic = "routines" | "media" | "artifacts" | "connectors";
const TOPICS: LiveTopic[] = ["routines", "media", "artifacts", "connectors"];

export class Live {
  private ws: WebSocket | null = null;
  private views = new Map<string, SessionView>();
  private watchers = new Map<string, number>();
  private listeners = new Set<Listener>();
  private topics = new Map<LiveTopic, Set<Listener>>(TOPICS.map((t) => [t, new Set()]));
  private backoff = 500;
  /** The wait before knocking again after a 4401 whose login proved good. */
  private endedWait = 500;
  private closed = false;
  private pending = false;
  private opened = false;
  // The fold loads with the chat, not with the first screen (it holds the
  // payload guard, and only a watched session needs it): events that come
  // before it are held, in order, and applied once it is here. In the app
  // the chat's screen loads the fold with itself (Shell.tsx), so it is here
  // before the first event; the hold is for a watch from elsewhere.
  private fold: Fold | null = null;
  private loading: Promise<unknown> | null = null;
  private refusals = new Set<string>();
  private failed = false;
  /** The fold's chunk (a seam for the tests of a failed load). */
  loadModule: () => Promise<Fold> = () => import("./fold");
  private early: RawEvent[] = [];
  state: ConnState = "connecting";
  /** Bumped on every change so React can cheaply detect updates. */
  version = 0;
  onSessionsChanged: () => void = () => {};
  onNodesChanged: () => void = () => {};
  onMemoryChanged: () => void = () => {};
  /** Workspaces, members, roles or the policy changed (content-free). */
  onWorkspacesChanged: () => void = () => {};
  onProjectsChanged: () => void = () => {};
  /** The server closed the socket for an ended login (4401, PLAN T2). It
   * also does so for this browser's own password change, which keeps its
   * login, so the listener checks before signing out. */
  onLoginEnded: () => void = () => {};

  constructor() {
    this.connect();
  }

  subscribe(l: Listener): () => void {
    this.listeners.add(l);
    return () => this.listeners.delete(l);
  }

  /** Call `fn` whenever `topic`'s list changes, and after a reconnect (it
   * may have changed meanwhile). Any number may listen: the shell and the
   * page showing the list. Returns the unsubscribe. */
  onTopic(topic: LiveTopic, fn: Listener): () => void {
    const set = this.topics.get(topic)!;
    set.add(fn);
    return () => {
      set.delete(fn);
    };
  }

  private fire(topic: LiveTopic) {
    this.topics.get(topic)!.forEach((fn) => fn());
  }

  private notify() {
    // Coalesce bursts of events into one render per frame.
    if (this.pending) return;
    this.pending = true;
    requestAnimationFrame(() => {
      this.pending = false;
      this.version++;
      this.listeners.forEach((l) => l());
    });
  }

  view(sessionId: string): SessionView | undefined {
    return this.views.get(sessionId);
  }

  /** The server answered a subscription to `sessionId` with "unknown
   * session" (crates/master/src/ws.rs): it does not exist or is not ours to
   * read, and no log will come. Cleared by the next `watch` of it. */
  refused(sessionId: string): boolean {
    return this.refusals.has(sessionId);
  }

  /** Follow a session. Counted: the chat and a sub-agent card may follow
   * the same one, and the subscription ends with the last `unwatch`. */
  watch(sessionId: string) {
    this.refusals.delete(sessionId);
    this.watchers.set(sessionId, (this.watchers.get(sessionId) ?? 0) + 1);
    this.loadFold();
    if (!this.views.has(sessionId)) {
      this.views.set(sessionId, emptyView());
      this.sendSubscribe(sessionId);
    }
  }

  unwatch(sessionId: string) {
    const n = (this.watchers.get(sessionId) ?? 1) - 1;
    if (n > 0) {
      this.watchers.set(sessionId, n);
      return;
    }
    this.watchers.delete(sessionId);
    if (this.views.delete(sessionId)) {
      this.send({ type: "unsubscribe", session_id: sessionId });
    }
  }

  close() {
    this.closed = true;
    this.ws?.close();
  }

  private loadFold() {
    if (this.fold) return;
    this.loading ??= this.loadModule().then(
      (m) => {
        this.fold = m;
        const early = this.early;
        this.early = [];
        for (const e of early) this.onEvent(e);
        // What came while it could not load: the server sends it again.
        if (this.failed) for (const id of this.views.keys()) this.sendSubscribe(id);
      },
      (err) => {
        // Tried again at the next watch, not per event (no request per
        // delta). In the app this is not reached: the chat's screen loads
        // the fold with itself, so a fold chunk that fails fails that screen
        // (ScreenBoundary says so), and React.lazy keeps that failure until
        // a reload. Kept for a watch that does not come from that screen.
        this.loading = null;
        this.failed = true;
        this.early = [];
        console.error(err);
      },
    );
  }

  private onEvent(e: RawEvent) {
    if (!this.fold) {
      if (this.loading) this.early.push(e);
      return;
    }
    const v = this.views.get(e.session_id);
    if (!v) return;
    if (e.seq <= v.head_seq) return; // duplicate
    if (e.seq > v.head_seq + 1) {
      this.sendSubscribe(e.session_id); // gap: ask again from our cursor
      return;
    }
    try {
      // Never throws for a payload (the fold's guard); only for a seq or
      // session that does not fit, checked above.
      this.fold.apply(v, e);
    } catch (err) {
      if (!(err instanceof this.fold.FoldError)) throw err;
      console.error(err);
    }
    this.notify();
  }

  private send(msg: unknown) {
    if (this.ws?.readyState === WebSocket.OPEN) this.ws.send(JSON.stringify(msg));
  }

  private sendSubscribe(sessionId: string) {
    const v = this.views.get(sessionId);
    if (v) this.send({ type: "subscribe", session_id: sessionId, after_seq: v.head_seq });
  }

  /** After a 4401 whose login proved good (`/api/me` answered): connect
   * again, but never at once. A tab whose cookie is good and whose socket is
   * still refused (a lost login key, a server that keeps closing it) would
   * otherwise ask `/api/me` and open a socket in a tight loop. The wait
   * doubles up to a minute and starts over once a socket has delivered a
   * message. */
  retryAfterLogin() {
    if (this.closed || this.ws) return;
    setTimeout(() => this.connect(), this.endedWait);
    this.endedWait = Math.min(this.endedWait * 2, 60_000);
  }

  connect() {
    if (this.closed || this.ws) return;
    const proto = location.protocol === "https:" ? "wss:" : "ws:";
    const ws = new WebSocket(`${proto}//${location.host}/api/ws`);
    this.ws = ws;
    this.setState("connecting");
    ws.onopen = () => {
      // The login key before anything else (P-1, api.ts): a socket without
      // it is closed as signed out, with nothing sent to it.
      ws.send(JSON.stringify({ type: "login", key: loginKey() ?? "" }));
      this.backoff = 500;
      this.setState("live");
      for (const id of this.views.keys()) this.sendSubscribe(id);
      // The list may have changed while we were away.
      this.onSessionsChanged();
      this.onNodesChanged();
      // So may the workspaces; the first connect comes with a fresh /api/me.
      // Each topic's listeners loaded their list when they subscribed.
      if (this.opened) {
        this.onWorkspacesChanged();
        TOPICS.forEach((t) => this.fire(t));
      }
      this.opened = true;
    };
    ws.onmessage = (m) => this.onMessage(JSON.parse(m.data as string));
    ws.onclose = (e) => {
      this.ws = null;
      const ended = e?.code === 4401;
      if (ended) this.onLoginEnded();
      this.setState("offline");
      // A signed-out socket does not knock again by itself: the shell checks
      // the login and calls `retryAfterLogin` if it still holds.
      if (!this.closed && !ended) {
        setTimeout(() => this.connect(), this.backoff);
        this.backoff = Math.min(this.backoff * 2, 5000);
      }
    };
  }

  private setState(s: ConnState) {
    this.state = s;
    this.notify();
  }

  private onMessage(msg: { type: string; event?: RawEvent; session_id?: string }) {
    // A socket that delivers is a socket the server accepted.
    this.endedWait = 500;
    switch (msg.type) {
      case "error":
        if (msg.session_id) {
          this.refusals.add(msg.session_id);
          this.notify();
        }
        break;
      case "event":
        this.onEvent(msg.event!);
        break;
      case "sessions_changed":
        this.onSessionsChanged();
        break;
      case "nodes_changed":
        this.onNodesChanged();
        break;
      case "memory_changed":
        this.onMemoryChanged();
        break;
      case "workspaces_changed":
        this.onWorkspacesChanged();
        break;
      case "projects_changed":
        this.onProjectsChanged();
        break;
      case "routines_changed":
      case "media_changed":
      case "artifacts_changed":
        this.fire(msg.type.slice(0, -"_changed".length) as LiveTopic);
        break;
      case "replayed":
        this.notify();
        break;
    }
  }
}

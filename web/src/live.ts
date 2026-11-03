// One WebSocket for everything. Each subscribed session keeps a cursor (its
// folded head_seq); on reconnect we resubscribe from that cursor, and the
// server replays exactly what we missed. Duplicates are dropped by seq, and a
// gap (which the server should never produce) triggers a resubscribe.
import { apply, emptyView, FoldError } from "./fold";
import type { RawEvent, SessionView } from "./types";

export type ConnState = "connecting" | "live" | "offline";

type Listener = () => void;

export class Live {
  private ws: WebSocket | null = null;
  private views = new Map<string, SessionView>();
  private listeners = new Set<Listener>();
  private backoff = 500;
  private closed = false;
  private pending = false;
  state: ConnState = "connecting";
  /** Bumped on every change so React can cheaply detect updates. */
  version = 0;
  onSessionsChanged: () => void = () => {};
  onNodesChanged: () => void = () => {};
  onMemoryChanged: () => void = () => {};

  constructor() {
    this.connect();
  }

  subscribe(l: Listener): () => void {
    this.listeners.add(l);
    return () => this.listeners.delete(l);
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

  watch(sessionId: string) {
    if (!this.views.has(sessionId)) {
      this.views.set(sessionId, emptyView());
      this.sendSubscribe(sessionId);
    }
  }

  unwatch(sessionId: string) {
    if (this.views.delete(sessionId)) {
      this.send({ type: "unsubscribe", session_id: sessionId });
    }
  }

  close() {
    this.closed = true;
    this.ws?.close();
  }

  private send(msg: unknown) {
    if (this.ws?.readyState === WebSocket.OPEN) this.ws.send(JSON.stringify(msg));
  }

  private sendSubscribe(sessionId: string) {
    const v = this.views.get(sessionId);
    if (v) this.send({ type: "subscribe", session_id: sessionId, after_seq: v.head_seq });
  }

  private connect() {
    if (this.closed) return;
    const proto = location.protocol === "https:" ? "wss:" : "ws:";
    const ws = new WebSocket(`${proto}//${location.host}/api/ws`);
    this.ws = ws;
    this.setState("connecting");
    ws.onopen = () => {
      this.backoff = 500;
      this.setState("live");
      for (const id of this.views.keys()) this.sendSubscribe(id);
      // The list may have changed while we were away.
      this.onSessionsChanged();
      this.onNodesChanged();
    };
    ws.onmessage = (m) => this.onMessage(JSON.parse(m.data as string));
    ws.onclose = () => {
      this.ws = null;
      this.setState("offline");
      if (!this.closed) {
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
    switch (msg.type) {
      case "event": {
        const e = msg.event!;
        const v = this.views.get(e.session_id);
        if (!v) return;
        if (e.seq <= v.head_seq) return; // duplicate
        if (e.seq > v.head_seq + 1) {
          this.sendSubscribe(e.session_id); // gap: ask again from our cursor
          return;
        }
        try {
          apply(v, e);
        } catch (err) {
          if (!(err instanceof FoldError)) throw err;
          console.error(err);
        }
        this.notify();
        break;
      }
      case "sessions_changed":
        this.onSessionsChanged();
        break;
      case "nodes_changed":
        this.onNodesChanged();
        break;
      case "memory_changed":
        this.onMemoryChanged();
        break;
      case "replayed":
        this.notify();
        break;
    }
  }
}

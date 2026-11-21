// An open session: header, timeline, reply composer. Loaded lazily so the
// new-tab home screen stays small.
import { own } from "../emptyView";
import { seqAt } from "../fold";
import "../lib/i18nPages";
import { errText } from "../lib/errorText";
import { AtSign, Download, FileText, Image as ImageIcon, Package, Square, Trash2 } from "lucide-react";
import { isValidElement, lazy, Suspense, useEffect, useLayoutEffect, useMemo, useReducer, useRef, useState, useSyncExternalStore, type ReactNode } from "react";
import { api } from "../api";
import { plansOf } from "../lib/planReview";
import { pageApi } from "../apiPages";
import { endWords, turnStart } from "../lib/callback";
import { useCompact } from "../lib/compact";
import { canRetry } from "../lib/retry";
import { changesFiles, forceable, noRewind, resend, runs, type Resend } from "../lib/rewind";
import { dropDraft, readDraft, writeDraft } from "../lib/drafts";
import { useFeatures } from "../lib/features";
import { t } from "../lib/i18n";
import { fallbackNote } from "../lib/modelPlan";
import { markRunSeen } from "../lib/routineResults";
import { routineHash } from "../lib/routes";
import {
  citeMaps,
  citeSignature,
  groupWeb,
  provenanceAt,
  provenanceIndex,
  provenanceSignature,
  sourceKeys,
  webNotice,
} from "../lib/rich";
import type { Live } from "../live";
import { InputCtx, MessageCtx, ProgressCtx, type InputInfo, type Inputs, type MessageInfo } from "../md/context";
import { CopyMessage } from "../md/CopyMessage";
import { rt } from "../md/strings";
import { showHidden, shownName } from "../md/hidden";
import type { History } from "../bg/topics";
import type { Agent, ApprovalMode, HeadMoved, Item, ModelInfo, NodeInfo, RoutineMark, SessionRow, SessionView, UploadedAttachment } from "../types";
import { inputBlock, inputState, progressSignature, progressSlots, type ProgressSlots } from "../ui/blocks";
import { Composer, type ComposerHandle, type Submission } from "./Composer";
import type { Picture } from "../lib/downscale";
import { AgentMark, agentAlert } from "./AgentAvatar";
import { ChatArtifactsCtx, SaveAsArtifact, SavedItem, stagedKeys, useChatArtifacts } from "./ChatArtifacts";
import { BubbleImages } from "./ChatImages";
import { ChatTitle } from "./ChatTitle";
import { HeadNote, LaterNotes } from "./HeadNote";
import { ThinkingRow } from "./ThinkingRow";
import { DeleteChat } from "./DeleteChat";
import { useAvatarState } from "./avatarState";
import { useSubagentOnly } from "../lib/useSubagentOnly";
import { Markdown } from "./Markdown";
import { Mentioned } from "./Mentioned";
import { peersLeft } from "../lib/peers";
import { NodeBadge } from "./NodeBadge";
import { ExecBadge } from "./ExecBadge";
import { compactTokens, ContextMeter } from "./ContextMeter";
import { CostMeter } from "./CostMeter";
import { RetryNote } from "./RetryNote";
import { ForkChat, ForkDivider, ForkNote, InheritedChanges } from "./ForkChat";
import { canFork } from "../lib/fork";
import { RewindMessage } from "./RewindMessage";
import { RewoundBlock } from "./RewoundBlock";
import { TurnStatus, turnState } from "./TurnStatus";
import { type Outgoing, outbox } from "../lib/outbox";
import { useHeaderFigures } from "../lib/childcost";
import { LiveCtx } from "../lib/liveCtx";
import { repliedBlock, ReplyBubble } from "./ReplyBubble";
import { ApprovalChoice, Held } from "./ApprovalChoice";
import { ToolCard, WebGroupCard } from "./ToolCard";
import { RowBoundary, RowFailed } from "./RowBoundary";
import { canJoinProject, ProjectChip } from "./ProjectChip";
import type { ProjectSummary } from "../lib/projects";

// Opt-in, and only its own small chunk: most people never load it.
const ChatGutter = lazy(() => import("../bg/ChatGutter"));
// Work beside the conversation (sub-agents, background commands): loaded on
// the chat's first one.
const SidePanel = lazy(() => import("./SidePanel"));
// A turn's changes (X.12), beside the conversation: loaded on the first
// "Show changes".
const DiffPanel = lazy(() => import("./DiffPanel"));
// Export as Markdown, loaded on its first click.
const ExportChat = lazy(() => import("./ExportChat"));
// Loaded on the chat's first task list (the agent's `todo` tool).
const TodoPanel = lazy(() => import("./TodoPanel"));
// Plan mode's bar (X.7), loaded on the first chat in plan mode.
const PlanBar = lazy(() => import("./PlanBar"));
const PlanReview = lazy(() => import("./PlanReview"));

/** Whether the chat has anything the side panel could list: its chunk loads
 * on the first sub-agent or background command. Cheap; builds no rows. */
function mayHaveSideItems(view: SessionView | undefined): boolean {
  if (!view) return false;
  if (view.background && Object.keys(view.background).length > 0) return true;
  // Calls allowed for the rest of the chat are listed there too (T51).
  if (view.chat_allows && Object.keys(view.chat_allows).length > 0) return true;
  return view.items.some((it) => it.type === "tool" && it.tool === "delegate");
}

/** How HEAD moved in each turn after item `i` that a rewind would undo. */
function laterMoves(view: SessionView, i: number): HeadMoved[] {
  const out: HeadMoved[] = [];
  for (const x of view.items.slice(i + 1)) {
    if (x.type !== "turn_end") continue;
    const c = own(view.checkpoints, x.turn_id);
    if (c?.head_moved && !c.restored) out.push(c.head_moved);
  }
  return out;
}

function Undo(props: {
  sessionId: string;
  turnId: string;
  files: string[];
  restored: boolean;
  running: boolean;
  /** Later turns also changed files: offer to rewind them too. */
  laterChanges: boolean;
  /** The viewer may not drive this chat here (row.blocked): the list of
   * files stays, the server would refuse an undo. */
  blocked: boolean;
  /** Open the turn's changes beside the chat (this turn's, or with
   * `rewind` this one's and every later one's). */
  onShow: (rewind: boolean, from: HTMLElement) => void;
  /** How the repository's HEAD moved in this turn, when it did (X.42). */
  headMoved?: HeadMoved;
  /** The same for each later turn a rewind would also undo. */
  laterMoves?: HeadMoved[];
  /** After an undo: the checkpoint it saved first. Restoring it puts back
   * what the undo replaced (an undo of the undo: the node's same checks). */
  saved?: string;
}) {
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  // What the last click asked, so "Undo anyway" repeats exactly that.
  const [last, setLast] = useState<{ turn: string; rewind: boolean }>({ turn: props.turnId, rewind: false });
  // The node said the saved checkpoint is not kept any more.
  const [gone, setGone] = useState(false);
  const run = async (force: boolean, ask = last) => {
    setBusy(true);
    setErr(null);
    setLast(ask);
    try {
      await pageApi.restoreTurn(props.sessionId, ask.turn, force, ask.rewind);
    } catch (x) {
      const m = x instanceof Error ? x.message : String(x);
      if (ask.turn !== props.turnId && m.includes("no longer kept")) setGone(true);
      setErr(m);
    } finally {
      setBusy(false);
    }
  };
  const mine = { turn: props.turnId, rewind: false };
  const putBack = props.restored && props.saved && !gone && !props.blocked && (
    <button
      type="button"
      className="link"
      disabled={busy || props.running}
      title="Restore the folder as it was just before the undo: the files it replaced come back, with the same checks as any undo"
      onClick={() => void run(false, { turn: props.saved!, rewind: false })}
    >
      Put back what this undo replaced
    </button>
  );
  const failed = err && (
    <div className="error-text">
      {err}{" "}
      {/* Every refusal a forced undo goes past says so. */}
      {forceable(err) && (
        <button type="button" className="link" onClick={() => void run(true)}>
          Undo anyway
        </button>
      )}
    </div>
  );
  return (
    <div className="undo small">
      <details>
        <summary>
          {props.files.length} file{props.files.length === 1 ? "" : "s"} changed{props.restored ? " · undone" : ""}
        </summary>
        <ul className="mono">
          {props.files.map((f) => (
            <li key={f}>{f}</li>
          ))}
        </ul>
      </details>
      {props.headMoved && <HeadNote moved={props.headMoved} />}
      {!!props.laterMoves?.length && props.laterChanges && !props.blocked && <LaterNotes moves={props.laterMoves} />}
      <button
        type="button"
        className="link"
        title="See what this turn changed, line by line, beside the chat"
        onClick={(e) => props.onShow(props.files.length === 0, e.currentTarget)}
      >
        Show changes
      </button>
      {!props.restored && !props.blocked && (
        <button type="button" className="link" disabled={busy || props.running} onClick={() => void run(false, mine)}>
          Undo file changes
        </button>
      )}
      {putBack}
      {props.laterChanges && !props.blocked && (
        <button
          type="button"
          className="link"
          disabled={busy || props.running}
          title="Undo this turn and every later turn's file changes"
          onClick={() => void run(false, { turn: props.turnId, rewind: true })}
        >
          Rewind files to before this turn
        </button>
      )}
      {failed}
    </div>
  );
}

/** Retry on a failed turn's end line (X.21): the node runs the same message
 * again with the chat's current agent and model, after the failed
 * attempt's steps, which stay above. The route refuses what this misses. */
function RetryTurn(props: { sessionId: string; turnId: string }) {
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const run = async () => {
    setBusy(true);
    setErr(null);
    try {
      await pageApi.retryTurn(props.sessionId, props.turnId);
    } catch (x) {
      setErr(errText(x));
      setBusy(false);
    }
  };
  return (
    <>
      {" · "}
      <button
        type="button"
        className="link"
        disabled={busy}
        title="Run your message again with this chat's agent and model; what this attempt did stays, and the agent is told"
        onClick={() => void run()}
      >
        Retry
      </button>
      {err && <div className="error-text">{err}</div>}
    </>
  );
}

/** What ui blocks need from the conversation: where each progress block
 * stands, and whether each question or form can be answered. */
function BlockCtx(props: { slots: ProgressSlots; inputs: Inputs; children: ReactNode }) {
  return (
    <ProgressCtx.Provider value={props.slots}>
      <InputCtx.Provider value={props.inputs}>{props.children}</InputCtx.Provider>
    </ProgressCtx.Provider>
  );
}

/** Each fallback note by the reply it belongs to (the turn's first reply
 * after it); a note whose turn gave no reply stays where it is. */
function fallbackNotes(items: Item[]): { byReply: Map<number, Extract<Item, { type: "fallback" }>>; attached: Set<number> } {
  const byReply = new Map<number, Extract<Item, { type: "fallback" }>>();
  const attached = new Set<number>();
  let pending: number | null = null;
  items.forEach((it, i) => {
    if (it.type === "fallback") pending = i;
    else if (turnStart(items, i) || it.type === "turn_end") pending = null;
    else if (it.type === "assistant" && pending !== null) {
      byReply.set(i, items[pending] as Extract<Item, { type: "fallback" }>);
      attached.add(pending);
      pending = null;
    }
  });
  return { byReply, attached };
}

function FallbackNote(props: { note: Extract<Item, { type: "fallback" }> }) {
  const { text, title } = fallbackNote(props.note);
  return (
    <div className="fallback-note" title={title || undefined}>
      {text}
    </div>
  );
}

/** A routine run's chat on screen, in a visible page, is seen once the run
 * has finished (marking a run still going changes nothing on the server). */
function useRunSeen(mark: RoutineMark | null | undefined) {
  const due = !!mark && !mark.seen && !["pending", "running", "waiting"].includes(mark.status);
  const routineId = mark?.routine_id;
  const runId = mark?.run_id;
  useEffect(() => {
    if (!due || !routineId || !runId) return;
    const seen = () => {
      if (document.visibilityState === "visible") void markRunSeen(routineId, runId).catch(() => {});
    };
    seen();
    document.addEventListener("visibilitychange", seen);
    return () => document.removeEventListener("visibilitychange", seen);
  }, [due, routineId, runId]);
}

/** A chat takes this many images in all (docs/media.md §6.6). */
const CHAT_IMAGES = 50;
/** An approval mode in the chat's own words (T50). */
const MODE_NAMES: Record<ApprovalMode, string> = { plan: "plan", ask_all: "ask every call", ask: "ask", accept_edits: "accept edits", auto: "auto" };

function compact(n: number): string {
  return n >= 1000 ? `${(n / 1000).toFixed(n >= 10000 ? 0 : 1)}k` : String(n);
}

export default function Conversation(props: {
  live: Live;
  sessionId: string;
  row: SessionRow | null;
  nodes: NodeInfo[];
  agents: Agent[];
  models: ModelInfo[];
  voice?: boolean;
  /** "Also in chats" is on: the history the gutter's wall mixes by. */
  gutter?: History | null;
  /** The workspace's projects, for the header's project picker. */
  projects?: ProjectSummary[];
}) {
  const { live, sessionId } = props;
  useEffect(() => {
    live.watch(sessionId);
    return () => live.unwatch(sessionId);
  }, [live, sessionId]);
  const view = live.view(sessionId);
  // The header's figures: the log's, plus the running foreground sub-agents'.
  const figures = useHeaderFigures(live, sessionId, view, props.models);
  const heldRun: ReactNode[] = [];
  /** One item, in its own boundary: a row that throws costs that row, not the chat (RowBoundary). */
  const row = (it: Item, i: number, draw: () => ReactNode) => {
    let el: ReactNode;
    try {
      el = draw();
    } catch (e) {
      console.error(e);
      el = <RowFailed item={it} sessionId={sessionId} canAnswer={!blocked} />;
    }
    // Taken out of the conversation by a rewind (X.46): the items of a run
    // are held, nothing in them is acted on (`inert`), and the run's last
    // one draws them as one closed block.
    if (it.rewound !== undefined) return rewoundRow(it, i, el);
    if (el == null || el === false) return null;
    const key = isValidElement(el) && el.key != null ? el.key : i;
    return (
      <RowBoundary key={key} item={it} sessionId={sessionId} canAnswer={!blocked}>
        {el}
      </RowBoundary>
    );
  };
  const rewoundRow = (it: Item, i: number, el: ReactNode): ReactNode => {
    if (el != null && el !== false) {
      const key = isValidElement(el) && el.key != null ? el.key : i;
      heldRun.push(
        <div key={key} inert>
          <RowBoundary item={it} sessionId={sessionId} canAnswer={false}>
            {el}
          </RowBoundary>
        </div>,
      );
    }
    const run = view && rewoundEnds.get(i);
    if (!view || !run) return null;
    const info = view.rewinds?.find((r) => r.seq === run.rewind);
    const first = view.items[run.start];
    const seq = seqAt(view, run.start);
    // The message the rewind went back to leads its run; what it can be sent again with.
    const said = info?.can_undo && first?.type === "user" && seq === info.message_seq ? resend(first) : null;
    // The files this rewind took back and nobody put back yet: the restore
    // it made saved the folder first, and its turns carry that id (the fold's
    // checkpoints, as the Undo block reads them). Computed here, per render.
    let filesBack: string | undefined;
    if (info?.files && !blocked) {
      for (let k = run.start; k <= run.end && !filesBack; k++) {
        const x = view.items[k]!;
        const c = x.type === "turn_end" ? own(view.checkpoints, x.turn_id) : undefined;
        if (c?.restored && c.saved) filesBack = c.saved;
      }
    }
    return (
      <RewoundBlock
        key={`rw${run.rewind}-${run.start}`}
        sessionId={sessionId}
        rewind={run.rewind}
        turns={run.turns}
        canUndo={!!info?.can_undo && !blocked && !running && !!props.row && view.items.findLastIndex((x) => x.rewound === run.rewind) === i}
        filesBack={filesBack}
        running={running}
        lost={said?.lost ?? []}
        putBack={said && (!said.text || !boxText.current.includes(said.text)) ? () => putRewound(said) : undefined}
      >
        {heldRun.splice(0)}
      </RewoundBlock>
    );
  };
  const scroller = useRef<HTMLDivElement>(null);
  const stick = useRef(true);

  useLayoutEffect(() => {
    const el = scroller.current;
    if (el && stick.current) el.scrollTop = el.scrollHeight;
  });

  const meta = view?.meta;
  const running = !!view?.running_turn;
  const summarise = useCompact(sessionId, view);
  const items = view?.items ?? [];
  // Where each run of rewound items ends: one pass per render (the items are mutated in place).
  const rewoundEnds = new Map(runs(items).map((r) => [r.end, r]));
  // The view is mutated in place, so memoise on what the result depends on.
  const provSig = provenanceSignature(items);
  const firstSeen = useMemo(() => provenanceIndex(items), [view, provSig]);
  const citeSig = citeSignature(items);
  const cites = useMemo(() => citeMaps(items), [view, citeSig]);
  // One stable context value per message, so streaming does not re-render links.
  const infoAt = useMemo(() => {
    const cache = new Map<number, MessageInfo>();
    return (i: number): MessageInfo => {
      let info = cache.get(i);
      if (!info) {
        const c = cites(i);
        info = { provenance: provenanceAt(firstSeen, i), sources: sourceKeys(c), cites: c };
        cache.set(i, info);
      }
      return info;
    };
  }, [firstSeen, cites]);
  // Which progress blocks are the turn's live card and which are snapshots.
  const progressSig = view ? progressSignature(items, view) : "";
  const slots = useMemo(() => (view ? progressSlots(items, view) : new Map()), [view, progressSig]);
  // Whether each question or form can be answered now, and what the agent
  // read before asking it. It changes with what progressSlots watches and
  // when a web result arrives.
  const rowIncognito = props.row?.incognito;
  const inputs = useMemo((): Inputs => {
    const byMessage = new Map<string, InputInfo>();
    if (view) {
      const incognito = !!(view.meta?.incognito ?? rowIncognito);
      items.forEach((it, i) => {
        if (it.type !== "assistant" || !inputBlock(it)) return;
        byMessage.set(it.message_id, { state: inputState(items, i, view), notice: webNotice(items, i), incognito });
      });
    }
    return { sessionId, byMessage };
  }, [view, progressSig, citeSig, sessionId, rowIncognito]);
  // Runs of web tool calls show as one "Researched" row; the rest of a run
  // is rendered by its first item.
  const webGroups = groupWeb(items);
  let groupedUntil = -1;
  // Items before the running turn's user message are final even if their
  // stream was cut off (a cancelled, failed or interrupted turn leaves
  // done=false); such a message is shown as cut off.
  const liveFrom = running ? items.findLastIndex((_, i) => turnStart(items, i)) : Infinity;
  const nodeId = meta?.node_id ?? props.row?.node_id ?? "";
  const node = props.nodes.find((n) => n.node_id === nodeId);
  const agentic = (meta?.profile ?? props.row?.profile) === "agentic";
  // The chat's agent: the person's pick (its row), which the next message runs.
  const activeAgent = props.row?.agent_id ?? meta?.agent_id ?? "general";
  const agentName = (id: string | null | undefined) =>
    props.agents.find((a) => a.id === id)?.name ?? id ?? "";
  const incognito = !!(meta?.incognito ?? props.row?.incognito);
  // A draft only once the chat is known not to be incognito; an incognito
  // chat loses one it had (AUDIT L14).
  const keepsDraft = !!(meta || props.row) && !incognito;
  useEffect(() => {
    if (incognito) dropDraft(sessionId);
  }, [incognito, sessionId]);
  // Why the viewer cannot drive this agentic chat here: approvals offer Deny
  // only and the reply box is read-only.
  const blocked = props.row?.blocked ?? null;
  // The face of whoever answers now; the header's agent shows it unless an
  // @mention has another agent answering.
  const face = useAvatarState(view);
  // This tab's messages the log does not hold yet (lib/outbox).
  useSyncExternalStore(outbox.subscribe, outbox.version);
  const users = items.flatMap((it) => (it.type === "user" ? [it.text] : []));
  const sending = outbox.unsent(sessionId, users);
  const held = sending.filter((o) => o.failed);
  useEffect(() => outbox.prune(sessionId, users));
  const box = useRef<ComposerHandle | null>(null);
  const alive = useRef(true);
  useEffect(() => () => void (alive.current = false), []);
  // A refused message whose own box is gone (a new chat's first one): its
  // text, attachments and images go into this box, ahead of what is typed,
  // once; the bubble stays, with Retry and Edit.
  const putBack = useRef(new Set<number>());
  useEffect(() => {
    for (const o of held) if (!putBack.current.has(o.id)) putBack.current.add(o.id), put(o, false);
  }, [held.map((o) => o.id).join()]);
  const fill = (text: string, files: UploadedAttachment[], pics: Picture[], take: boolean) => {
    const b = box.current;
    b?.setText((c) => (take ? c.replace(text, "").trim() : c.includes(text) ? c : c ? `${text}\n${c}` : text));
    b?.setFiles((f) => [...(take ? [] : files), ...f.filter((x) => !files.some((y) => y.attachment_id === x.attachment_id))]);
    b?.setPictures((p) => [...(take ? [] : pics), ...p.filter((x) => !pics.some((y) => y.key === x.key))]);
  };
  const put = (o: Outgoing, take: boolean) => fill(o.text, o.s?.uploads ?? [], o.s?.pictures ?? [], take);
  // A rewind's message goes back into the box (X.46): by itself only into an
  // empty one (never over what is typed); on request, merged as a refused
  // message is. The messages (by seq) whose box already has them.
  // What the box holds, by its own report; the chat redraws on a change only
  // while a rewind could still offer the message ("Put the message back in
  // the box" is offered only when the box does not hold its text).
  const boxText = useRef("");
  const watchBox = useRef(false);
  const [, redraw] = useReducer((n: number) => n + 1, 0);
  const onBoxText = (x: string) => {
    if (boxText.current === x) return;
    boxText.current = x;
    if (watchBox.current) redraw();
  };
  watchBox.current = !!view?.rewinds?.some((r) => r.can_undo);
  const putRewound = (m: Resend) => {
    fill(m.text, [], m.pictures, false);
    box.current?.focus();
  };
  // A rewind undone (by this tab, or seen arriving from another): the message
  // is in the chat again, so a box that still holds exactly what the rewind
  // put there (its text and pictures, nothing attached) is emptied, or Enter
  // would send it twice. Edited, it stays. Rewinds already undone when the chat
  // opened are not acted on. Runs after every render: the view is mutated in place.
  const undoneSeen = useRef<{ sid: string; seqs: Set<number> } | null>(null);
  useEffect(() => {
    if (!view) return;
    const now = (view.rewinds ?? []).filter((r) => r.undone);
    const seen = undoneSeen.current;
    if (!seen || seen.sid !== sessionId) {
      undoneSeen.current = { sid: sessionId, seqs: new Set(now.map((r) => r.seq)) };
      return;
    }
    for (const r of now) {
      if (seen.seqs.has(r.seq)) continue;
      seen.seqs.add(r.seq);
      const at = view.items.findIndex((x, k) => x.type === "user" && seqAt(view, k) === r.message_seq);
      const item = view.items[at];
      const m = item?.type === "user" ? resend(item) : null;
      const held = box.current?.content();
      if (!m || !held || held.files > 0) continue;
      const keys = m.pictures.map((p) => p.key).sort();
      if (held.text.trim() === m.text.trim() && held.pictures.length === keys.length && [...held.pictures].sort().every((k, n) => k === keys[n])) {
        box.current?.setText("");
        box.current?.setPictures([]);
      }
    }
  });
  const afterRewind = (m: Resend | null) => {
    if (m && box.current?.empty()) putRewound(m);
    else box.current?.focus();
  };
  // One send: shown at once; the log's message replaces it (lib/outbox).
  // `again`: a Retry, whose refusal is shown on its bubble, not in the box.
  const send = async (s: Submission, again = false) => {
    stick.current = true;
    const o = outbox.add(sessionId, s.text, users.length, s);
    try {
      await api.prompt(sessionId, s.text, {
        ...(s.effort ? { effort: s.effort } : {}),
        ...(s.attachments.length ? { attachments: s.attachments } : {}),
        ...(s.images?.length ? { images: s.images } : {}),
        ...(s.files?.length ? { files: s.files } : {}),
        ...(s.compareWith ? { compare: [s.model, s.compareWith] } : {}),
      });
      outbox.accept(o);
      // Sent from a box that is gone: the draft kept it until now.
      if (!alive.current && keepsDraft) writeDraft(sessionId, readDraft(sessionId).replace(s.text, "").trim());
    } catch (x) {
      // The reply box takes the text back and says why; the draft kept it
      // all along, for a box that is gone (another chat opened meanwhile).
      outbox.drop(sessionId, o, again ? (x instanceof Error ? x.message : String(x)) : undefined);
      if (!again) throw x;
    }
  };
  const answering = view?.running_agent?.agent_id ?? activeAgent;
  const facing = props.agents.find((a) => a.id === answering);
  const sessionAgent = props.agents.find((a) => a.id === activeAgent);
  const subagentOnly = useSubagentOnly(!!props.row?.parent_id, props.row?.workspace_id ?? null, activeAgent);
  const notes = fallbackNotes(items);
  // Calls waiting for an answer, in the order the node asked: the first
  // one's choice takes the composer's place (row W.6). Where there is no
  // composer (a sub-agent that takes tasks only) the cards keep their buttons.
  const holding = subagentOnly
    ? []
    : (view?.pending_approvals ?? []).flatMap((id) => items.filter((x): x is Extract<typeof x, { type: "tool" }> => x.type === "tool" && x.approval?.approval_id === id));
  const choiceBox = useRef<HTMLDivElement>(null);
  const routine = props.row?.routine;
  useRunSeen(routine);
  // "Rewind to here" on the person's message (X.46): the fold's data decides, the server decides again.
  const rewindable = (v: SessionView, i: number) =>
    noRewind(v, i, {
      mine: !!props.row && !blocked,
      subagent: !!props.row?.parent_id || !!meta?.parent_session,
      routine: !!routine,
      compacting: summarise.compacting,
    }) === null;
  // What the server offers here: "Save as artifact" in agentic chats, images in any.
  const workspaceId = props.row?.workspace_id ?? null;
  const features = useFeatures(workspaceId, !!props.row && !blocked && !incognito);
  const saveable = agentic && !!features?.artifacts.enabled;
  const imagesUsed = items.reduce((n, it) => n + (it.type === "user" ? (it.images?.length ?? 0) : 0), 0);
  const artifacts = useChatArtifacts(sessionId, live, stagedKeys(items), saveable);
  const [savingFile, setSavingFile] = useState(false);
  // Deleting asks in a dialog that also offers the chat's Media files.
  const [deleting, setDeleting] = useState(false);
  // Export as Markdown: its dialog and the file's builder load on the first click.
  const [exporting, setExporting] = useState(false);
  const remove = () => setDeleting(true);
  // The background strip: once a command of this chat runs, it stays
  // mounted (it hides itself) so an end this tab asked for can show.
  const sideFor = useRef<string | null>(null);
  if (mayHaveSideItems(view)) sideFor.current = sessionId;
  const todoFor = useRef<string | null>(null);
  if (view?.todo?.length) todoFor.current = sessionId;
  // The turn whose changes are open beside the chat, and the row's button
  // that opened them (focus goes back to it on close).
  // Plan mode's bar: a send that failed after the mode changed.
  const [carryErr, setCarryErr] = useState<string | null>(null);
  // The plan-review pane (PlanReview.tsx) over the chat, and what to focus when it closes.
  const [reviewing, setReviewing] = useState(false);
  const reviewOpener = useRef<HTMLElement | null>(null);
  useEffect(() => setReviewing(false), [sessionId]);
  const closePlanReview = () => {
    setReviewing(false);
    reviewOpener.current?.focus();
  };
  // Not memoised: the view's `items` array is mutated in place, so its identity
  // says nothing; one pass over the log per render, like the signatures above.
  const plans = plansOf(items, meta?.approval_mode);
  // Switch the mode, then send the person's message (plan bar and review pane).
  const carryOut = async (m: ApprovalMode, text: string) => {
    setCarryErr(null);
    await pageApi.setApprovalMode(sessionId, m);
    stick.current = true;
    // The bar is gone once the mode changed: a failed send is
    // said here, where it stays.
    await api.prompt(sessionId, text, {}).catch((x) => {
      setCarryErr(`The mode changed, but "${text}" was not sent: ${x instanceof Error ? x.message : String(x)}`);
    });
  };
  // The person's pick: shown at once and held (the picker disabled) until
  // the server answered, one at a time so the last pick wins; dropped on a
  // refusal (said under the bar) or once the chat's row names an agent.
  const [agentPick, setAgentPick] = useState<[string, boolean] | null>(null);
  const [pickErr, setPickErr] = useState<string | null>(null);
  const rowAgent = props.row?.agent_id;
  useEffect(() => setAgentPick(null), [rowAgent, sessionId]);
  useEffect(() => setPickErr(null), [sessionId]);
  useEffect(() => setCarryErr(null), [sessionId]);
  const [diffOf, setDiffOf] = useState<{ session: string; turn: string; rewind: boolean } | null>(null);
  const diffOpener = useRef<HTMLElement | null>(null);
  const closeDiff = () => {
    setDiffOf(null);
    diffOpener.current?.focus();
  };

  const page = (
    <section className={props.gutter ? "conversation with-gutter" : "conversation"}>
      {props.gutter && (
        <Suspense fallback={null}>
          <ChatGutter sessionId={sessionId} agentic={agentic} history={props.gutter} />
        </Suspense>
      )}
      <header className="conv-head">
        <div className="conv-title">
          {/* The row's title: a rename changes only the row (the log keeps the first). */}
          <ChatTitle sessionId={sessionId} title={props.row?.title ?? meta?.title ?? "…"} canRename={!!props.row} />
          <div className="conv-meta">
            <span className={`conv-agent${sessionAgent && agentAlert(sessionAgent) ? " bad" : ""}`}>
              {sessionAgent && <AgentMark agent={sessionAgent} state={answering === activeAgent ? face : "idle"} size={20} />}
              <span className="ellipsis">{agentName(activeAgent)}</span>
            </span>
            <span className="muted meta-model">{meta?.model}</span>
            {agentic && <NodeBadge node={node} nodeId={nodeId} />}
            {agentic && <ExecBadge mode={meta?.exec_mode} node={node} />}
            {agentic && <span className="mono muted ellipsis">{shownName(meta?.cwd ?? "")}</span>}
            {/* A fresh fork has spent nothing but will send the copied context: show both. */}
            {view && figures && (figures.usage.input_tokens + figures.usage.output_tokens > 0 || view.context_tokens > 0) && (
              <span
                className="muted meter"
                title={`${figures.usage.input_tokens.toLocaleString()} in · ${figures.usage.output_tokens.toLocaleString()} out${
                  figures.usage.cache_read_tokens
                    ? ` · ${figures.usage.cache_read_tokens.toLocaleString()} of the input read from the provider's cache`
                    : ""
                }`}
              >
                {compact(figures.usage.input_tokens + figures.usage.output_tokens)} {t("conv.tokens")} ·{" "}
                <CostMeter usage={figures.costUsage} turn={figures.costTurn} models={props.models} />
                <ContextMeter used={view.context_tokens} window={view.context_window} summaryAt={view.summary_at} model={view.context_model ?? meta?.model ?? ""} />
              </span>
            )}
          </div>
        </div>
        {props.row && canJoinProject(props.row) && <ProjectChip row={props.row} projects={props.projects ?? []} />}
        {saveable && (
          <button
            type="button"
            className="icon"
            aria-label="Save a file as an artifact"
            title="Save a file as an artifact"
            aria-expanded={savingFile}
            onClick={() => setSavingFile(!savingFile)}
          >
            <Package size={18} />
          </button>
        )}
        <button
          type="button"
          className="icon"
          aria-label={t("conv.export")}
          title={t("conv.export")}
          disabled={!view || view.items.length === 0}
          onClick={() => setExporting(true)}
        >
          <Download size={18} />
        </button>
        <button type="button" className="icon" aria-label={t("conv.delete")} title={t("conv.delete")} onClick={remove}>
          <Trash2 size={18} />
        </button>
      </header>
      {deleting && <DeleteChat sessionId={sessionId} workspaceId={workspaceId} incognito={incognito} onClose={() => setDeleting(false)} />}
      {exporting && view && (
        <Suspense fallback={null}>
          <ExportChat
            view={view}
            title={props.row?.title ?? meta?.title ?? ""}
            agentName={(id) => agentName(id ?? activeAgent)}
            onClose={() => setExporting(false)}
          />
        </Suspense>
      )}
      {savingFile && (
        <div className="incognito-banner save-file-bar">
          <SaveAsArtifact autoFocus />
        </div>
      )}
      {routine?.untrusted && (
        <div className="incognito-banner untrusted-banner" role="note">
          <span>Contains data from an external event, so memory and web tools stay off in this chat.</span>
          <a className="link" href={routineHash(routine.routine_id)}>
            {routine.name}
          </a>
        </div>
      )}
      {props.row?.parent_id && (
        <div className="incognito-banner">
          <span>
            Sub-task run by {agentName(activeAgent)} for another session.
          </span>
          <a className="link" href={`#/s/${props.row.parent_id}`}>
            Back to the main session
          </a>
        </div>
      )}
      {incognito && (
        <div className="incognito-banner">
          <span>{t("conv.incognito")}</span>
          <button type="button" className="link" onClick={remove}>
            {t("conv.deleteNow")}
          </button>
        </div>
      )}

      <div className="conv-body">
        <div className="conv-main">
      <div
        className="timeline"
        ref={scroller}
        onScroll={() => {
          const el = scroller.current;
          if (el) stick.current = el.scrollHeight - el.scrollTop - el.clientHeight < 80;
        }}
      >
        <Held.Provider value={holding[0]?.approval?.approval_id ?? null}>
        <BlockCtx slots={slots} inputs={inputs}>
          <div className="column">
            {agentic && meta?.forked && <ForkNote parent={meta.forked.parent} />}
            {!view || view.head_seq === 0 ? (
              <div className="muted pad">Loading…</div>
            ) : (
              view.items.map((it, i) => row(it, i, () => {
                if (i <= groupedUntil) return null;
                const web = webGroups.get(i);
                if (web) {
                  groupedUntil = web.end;
                  return (
                    <WebGroupCard
                      key={i}
                      sessionId={sessionId}
                      group={web}
                      blocked={!!blocked}
                      cwd={meta?.cwd}
                      tools={web.members.map((m) => view.items[m] as Extract<typeof it, { type: "tool" }>)}
                    />
                  );
                }
                // Parallel answers: render the whole group once, side by side.
                if (it.type === "assistant" && it.variant) {
                  const g = it.variant.group;
                  const first = view.items.findIndex((x) => x.type === "assistant" && x.variant?.group === g);
                  if (first !== i) return null;
                  const answers = view.items.filter(
                    (x): x is Extract<typeof x, { type: "assistant" }> => x.type === "assistant" && x.variant?.group === g,
                  );
                  const chosen =
                    own(view.variant_selection, g) ?? answers.find((a) => a.variant?.index === 0)?.message_id;
                  const busy = running && view.running_turn === g;
                  return (
                    <div key={i} className="variants" style={{ gridTemplateColumns: `repeat(${answers.length}, minmax(0, 1fr))` }}>
                      {answers.map((a) => (
                        <div key={a.message_id} className={`msg assistant variant ${a.message_id === chosen ? "chosen" : ""}`}>
                          <div className="variant-head small">
                            <span className="mono">{a.variant?.model}</span>
                            {a.message_id === chosen ? (
                              <span className="muted">continuing from this</span>
                            ) : (
                              <button
                                type="button"
                                className="link"
                                disabled={busy}
                                onClick={() => void pageApi.selectVariant(sessionId, g, a.message_id)}
                              >
                                Continue with this
                              </button>
                            )}
                          </div>
                          <MessageCtx.Provider value={infoAt(i)}>
                            <Markdown
                              text={a.text}
                              done={a.done || !busy}
                              cut={!a.done && !busy}
                              blocks={a.blocks}
                              from={a.author ? agentName(a.author) : null}
                              messageId={a.message_id}
                            />
                          </MessageCtx.Provider>
                          {(a.done || !busy) && <CopyMessage text={a.text} />}
                        </div>
                      ))}
                    </div>
                  );
                }
                switch (it.type) {
                  case "user":
                    return (
                      <div key={i} className={it.queued && !it.queued.read ? "msg user waiting" : "msg user"}>
                        {it.reply ? (
                          <ReplyBubble text={it.text} reply={it.reply} block={repliedBlock(view.items, it.reply)} />
                        ) : (
                          <div className="bubble">
                            <Mentioned text={it.text} agents={props.agents} triggers={it.triggers} />
                          </div>
                        )}
                        {(it.attachments || it.files) && (
                          <ul className="chips">
                            {it.attachments?.map((n, j) => (
                              <li key={j} className="chip">
                                <FileText size={14} /> {n}
                              </li>
                            ))}
                            {it.files?.map((p, j) => (
                              <li key={`f${j}`} className="chip" title={t("conv.mentioned")}>
                                <AtSign size={14} /> {p}
                              </li>
                            ))}
                          </ul>
                        )}
                        {!!it.images?.length && <BubbleImages images={it.images} />}
                        {view.follow_ups?.includes(it.turn_id) && <div className="queued-mark muted">{t("conv.followUp")}</div>}
                        {rewindable(view, i) && (
                          <RewindMessage
                            sessionId={sessionId}
                            seq={seqAt(view, i)!}
                            files={changesFiles(view, i)}
                            onDone={() => afterRewind(resend(it))}
                          />
                        )}
                        {it.queued && !it.queued.read && (
                          <div className="queued-mark muted" title={t("conv.queuedHint")}>
                            {running ? t("conv.queued") : t("conv.queuedLater")}
                          </div>
                        )}
                      </div>
                    );
                  case "assistant": {
                    const final = it.done || i < liveFrom;
                    return (
                      <div key={i} className={`msg assistant ${!it.done && running ? "streaming" : ""}`}>
                        {it.author && it.author !== activeAgent && (
                          <div className="author">
                            {props.agents.some((a) => a.id === it.author) && (
                              <AgentMark agent={props.agents.find((a) => a.id === it.author)!} size={16} />
                            )}
                            {agentName(it.author)}
                          </div>
                        )}
                        <MessageCtx.Provider value={infoAt(i)}>
                          <Markdown
                            text={it.text}
                            done={final}
                            cut={final && !it.done}
                            blocks={it.blocks}
                            from={it.author ? agentName(it.author) : null}
                            messageId={it.message_id}
                          />
                        </MessageCtx.Provider>
                        {notes.byReply.has(i) && <FallbackNote note={notes.byReply.get(i)!} />}
                        {final && it.rewound === undefined && <CopyMessage text={it.text} />}
                      </div>
                    );
                  }
                  case "fallback":
                    return notes.attached.has(i) ? null : <FallbackNote key={i} note={it} />;
                  case "tool":
                    return (
                      <ToolCard
                        key={i}
                        sessionId={sessionId}
                        item={it}
                        blocked={!!blocked}
                        projectId={props.row?.project_id}
                        cwd={meta?.cwd}
                        inherited={meta?.forked != null && (seqAt(view, i) ?? Infinity) <= meta.forked.through_seq}
                      />
                    );
                  case "artifact":
                    return <SavedItem key={i} staged={it.staged} />;
                  case "artifact_failed":
                    return (
                      <div key={i} className="msg error">
                        Couldn't save {showHidden(it.path)}: {showHidden(it.error)}
                      </div>
                    );
                  case "error":
                    return (
                      <div key={i} className={it.notice ? "msg notice" : "msg error"}>
                        {showHidden(it.message)}
                      </div>
                    );
                  case "callback":
                    // Where the person's message would be: nobody wrote one.
                    return (
                      <div key={i} className="divider callback" role="note">
                        Started by {endWords(view.items.slice(0, i), it.bg_ids)}, not by you
                      </div>
                    );
                  case "called":
                    // Where the person's message would be: it called this agent
                    // too (the existing "Switched to" line is the chat's own
                    // agent changing, which this is not).
                    return (
                      <div key={i} className="divider" role="note">
                        {t("conv.calledBy", { name: it.agent ? `@${it.agent.agent_id}` : "an agent" })}
                      </div>
                    );
                  case "peers_skipped":
                    return (
                      <div key={i} className="divider" role="note">
                        {t("conv.notAsked", {
                          names: it.agents.map((a) => `@${shownName(a)}`).join(", "),
                          why: t(`conv.notAsked.${it.reason}`),
                        })}
                      </div>
                    );
                  case "retry":
                    // Where the person's message would be: they asked again.
                    return (
                      <div key={i} className="divider" role="note">
                        Retried
                        {it.model && it.failed_model && it.model !== it.failed_model
                          ? ` with ${it.model} (the attempt above used ${it.failed_model})`
                          : ""}
                      </div>
                    );
                  case "callback_refused":
                    return (
                      <div key={i} className="msg error">
                        {endWords(view.items.slice(0, i), it.bg_ids)}; the agent was not resumed: {showHidden(it.reason)}
                      </div>
                    );
                  case "background_ended":
                    // Program output: plain text, never rendered.
                    return (
                      <details key={i} className="divider">
                        <summary>
                          {it.agent ? `@${it.agent} ` : ""}
                          {it.bg_id} {it.open ? "may still be running" : "ended"} ({it.reason.replace(/_/g, " ")}
                          {it.exit_code != null ? `, code ${it.exit_code}` : ""}) after {Math.round(it.ran_ms / 1000)} s
                        </summary>
                        {it.tail && <pre>{showHidden(it.tail)}</pre>}
                      </details>
                    );
                  case "forked":
                    return <ForkDivider key={i} parent={it.parent} />;
                  case "agent_changed":
                    return (
                      <div key={i} className="divider">
                        Switched to {it.agent.name}
                        {it.model ? ` · ${it.model}` : ""}
                      </div>
                    );
                  case "approval_mode_changed":
                    return (
                      <div key={i} className="divider">
                        Approval mode: {own(MODE_NAMES, it.mode)}
                        {it.requested ? ` (asked for ${own(MODE_NAMES, it.requested)}; this node allows no more)` : ""}
                      </div>
                    );
                  case "step_limit_changed":
                    return (
                      <div key={i} className="divider">
                        Step limit: {it.max_steps}
                        {it.requested != null
                          ? ` (asked for ${it.requested}; this node allows 1 to ${node?.max_steps ?? "its own limit"})`
                          : ""}
                      </div>
                    );
                  case "summary":
                    // Only the latest summary is what the model sees now.
                    return view.items.findLastIndex((x) => x.type === "summary") !== i ? null : (
                      <details key={i} className="divider summary" data-testid="summary">
                        <summary>{t(it.manual ? "conv.summaryManual" : "conv.summary")}</summary>
                        {it.focus && <p>{t("conv.summaryFocus", { focus: it.focus })}</p>}
                        <p>{it.text}</p>
                      </details>
                    );
                  case "handoff":
                    // Only the latest note is what the next chat reads. The
                    // note is the model's text: shown as text.
                    return view.items.findLastIndex((x) => x.type === "handoff") !== i ? null : it.text ? (
                      <details key={i} className="divider summary" data-testid="handoff">
                        <summary>{t("conv.handoff", { model: it.model ?? "?" })}</summary>
                        <p>{it.text}</p>
                      </details>
                    ) : (
                      <div key={i} className="divider" data-testid="handoff">
                        {t("conv.handoffFixed", {
                          why: (it.failed ?? []).map((f) => `${f.model}: ${f.reason}`).join("; ") || "no model wrote one",
                        })}
                      </div>
                    );
                  case "thinking":
                    return <ThinkingRow key={i} it={it} />;
                  case "shortened": {
                    const what = [
                      it.cleared ? t("conv.shortCleared") : "",
                      it.steps ? t("conv.shortSteps", { n: String(it.steps) }) : "",
                      it.thinking ? t("conv.shortThinking") : "",
                    ]
                      .filter(Boolean)
                      .join(", ");
                    const line = t("conv.shortened", {
                      what,
                      before: compactTokens(it.tokens_before),
                      after: compactTokens(it.tokens_after),
                    });
                    // The summary is the model's text: shown as text.
                    return it.text ? (
                      <details key={i} className="divider summary">
                        <summary>{line}</summary>
                        <p>{it.text}</p>
                      </details>
                    ) : (
                      <div key={i} className="divider">
                        {line}
                      </div>
                    );
                  }
                  case "turn_end": {
                    const cp = own(view.checkpoints, it.turn_id);
                    const laterChanges = view.items.slice(i + 1).some((x) => {
                      if (x.type !== "turn_end") return false;
                      const c = own(view.checkpoints, x.turn_id);
                      return !!c && c.files.length > 0 && !c.restored;
                    });
                    // In a rewound turn nothing is acted on: the rewound block
                    // holds the way back for the files.
                    const gone = it.rewound !== undefined;
                    // A turn that changed no files has no Undo block, restored or not.
                    const undo =
                      // A turn copied from the chat this one was forked from: its
                      // files are named, with nothing to undo here.
                      !gone && cp && cp.after && cp.inherited && cp.files.length > 0 ? (
                        <InheritedChanges key={`u${i}`} files={cp.files} />
                      ) : !gone && cp && cp.after && !cp.inherited && cp.files.length > 0 ? (
                        <Undo
                          key={`u${i}`}
                          sessionId={sessionId}
                          turnId={it.turn_id}
                          files={cp.files}
                          restored={cp.restored}
                          running={running}
                          laterChanges={laterChanges}
                          blocked={!!blocked}
                          headMoved={cp.head_moved}
                          laterMoves={laterMoves(view, i)}
                          saved={cp.restored ? cp.saved : undefined}
                          onShow={(rewind, from) => {
                            diffOpener.current = from;
                            setDiffOf({ session: sessionId, turn: it.turn_id, rewind });
                          }}
                        />
                      ) : null;
                    // Not after an undo of its changes, nor where the viewer
                    // cannot drive the chat, a sub-agent's chat or a routine's
                    // own run (its first turn).
                    const retry =
                      !gone &&
                      !running &&
                      !blocked &&
                      !summarise.compacting &&
                      !cp?.restored &&
                      !props.row?.parent_id &&
                      !meta?.parent_session &&
                      !(routine && items.findIndex((x) => x.type === "turn_end") === i) &&
                      canRetry(items, i);
                    const fork = canFork(view, i, {
                      row: props.row,
                      node,
                      blocked: blocked ?? null,
                      routine: !!routine,
                      subAgent: !!props.row?.parent_id || !!meta?.parent_session,
                    }) ? (
                      <ForkChat sessionId={sessionId} turnId={it.turn_id} />
                    ) : null;
                    return it.reason === "completed" ? (
                      undo || fork ? (
                        <div key={i} className="turn-end">
                          {undo}
                          {fork}
                        </div>
                      ) : null
                    ) : (
                      <div key={i} className="turn-end">
                        <div className={`divider r-${it.reason}`}>
                          {it.reason === "limit" ? rt("ui.turn.limit") : `turn ${it.reason}`}
                          {retry && <RetryTurn sessionId={sessionId} turnId={it.turn_id} />}
                        </div>
                        {undo}
                        {fork}
                      </div>
                    );
                  }
                }
              }))
            )}
            {/* Events this page could not fold: a newer node's kind, or a payload of the wrong shape (fold.ts, the guard). */}
            {!!view?.unknown_events && (
              <div className="small muted pad" role="note" data-testid="unshown">
                {t("conv.unshown", { n: String(view.unknown_events) })}
              </div>
            )}
            {sending.map((o) => (
              <div key={`out${o.id}`} className={`msg user ${o.failed ? "failed" : o.ok ? "sent" : "sending"}`}>
                <div className="bubble">{o.text}</div>
                {!!(o.s?.uploads?.length || o.s?.files?.length || o.s?.pictures?.length) && (
                  <ul className="chips">
                    {o.s?.uploads?.map((f) => (
                      <li key={f.attachment_id} className="chip">
                        <FileText size={14} /> {f.name}
                      </li>
                    ))}
                    {o.s?.files?.map((p) => (
                      <li key={`f${p}`} className="chip">
                        <AtSign size={14} /> {p}
                      </li>
                    ))}
                    {o.s?.pictures?.map((p) => (
                      <li key={p.key} className="chip">
                        <ImageIcon size={14} /> {p.name}
                      </li>
                    ))}
                  </ul>
                )}
                {o.failed ? (
                  <div className="queued-mark sent-failed">
                    <span className="error-text">{t("conv.notSent", { why: o.failed })}</span>{" "}
                    <button
                      type="button"
                      className="link"
                      onClick={() => {
                        outbox.forget(sessionId, o);
                        put(o, true);
                        void send(o.s ?? ({ text: o.text, attachments: [] } as unknown as Submission), true);
                      }}
                    >
                      {t("conv.retrySend")}
                    </button>
                    {" · "}
                    <button
                      type="button"
                      className="link"
                      onClick={() => {
                        put(o, false);
                        outbox.forget(sessionId, o);
                      }}
                    >
                      {t("conv.editSend")}
                    </button>
                  </div>
                ) : (
                  !o.ok && <div className="queued-mark muted">{t("conv.sending")}</div>
                )}
              </div>
            ))}
            <TurnStatus
              state={turnState(view, sending.some((o) => !o.failed))}
              wait={`${view?.head_seq ?? 0}:${sending.length}`}
              turn={view?.running_turn ?? (sending.some((o) => !o.failed) ? "sending" : null)}
              name={agentName(answering)}
              mark={facing && <AgentMark agent={facing} state={face} size={22} />}
              left={peersLeft(view?.items ?? [], !!view?.running_turn)}
              link={live.state}
            />
            {running && view?.retry && <RetryNote retry={view.retry} />}
            {summarise.compacting && !running && (
              <div className="thinking" data-testid="compacting">
                {t("conv.compacting")}
              </div>
            )}
          </div>
        </BlockCtx>
        </Held.Provider>
      </div>

      <div className="bottom">
        <div className="column">
          {todoFor.current === sessionId && (
            <Suspense fallback={null}>
              <TodoPanel key={sessionId} items={view?.todo ?? []} running={running} />
            </Suspense>
          )}
          {meta?.approval_mode === "plan" && !running && !meta.parent_session && !meta.no_callbacks && node?.max_approval && (
            <Suspense fallback={null}>
              <PlanBar
                max={node.max_approval}
                onCarry={carryOut}
                onReview={
                  plans.length
                    ? () => {
                        reviewOpener.current = document.activeElement as HTMLElement | null;
                        setReviewing(true);
                      }
                    : undefined
                }
              />
            </Suspense>
          )}
          {carryErr && <div className="error-text">{carryErr}</div>}
          {reviewing && meta?.approval_mode === "plan" && node?.max_approval && (
            <Suspense fallback={null}>
              <PlanReview
                key={sessionId}
                sessionId={sessionId}
                plans={plans}
                running={running}
                max={node.max_approval}
                onCarry={carryOut}
                onSend={(text) =>
                  send({
                    text,
                    mode: "agentic",
                    agentId: activeAgent,
                    model: meta.model ?? "",
                    effort: null,
                    nodeId,
                    cwd: meta.cwd ?? "",
                    incognito: false,
                    attachments: [],
                    compareWith: null,
                  })
                }
                onClose={closePlanReview}
              />
            </Suspense>
          )}
          {pickErr && <div className="error-text">{pickErr}</div>}
          {holding[0] && (
            <ApprovalChoice
              key={holding[0].approval!.approval_id}
              sessionId={sessionId}
              item={holding[0]}
              blocked={!!blocked}
              cwd={meta?.cwd}
              place
              boxRef={choiceBox}
            />
          )}
          {subagentOnly ? (
            <div className="tag-only-row">
              <p className="muted small tag-only" role="note">
                {agentName(activeAgent)} only takes tasks from other agents, so there is nobody to answer here.
              </p>
              {/* The composer's Stop, for a sub-agent that goes on after its parent's turn ended. */}
              {running && (
                <button type="button" className="send stop" onClick={() => void pageApi.cancel(sessionId)} aria-label={t("composer.stop")}>
                  <Square size={16} fill="currentColor" />
                </button>
              )}
            </div>
          ) : (
          <Composer
            where="session"
            agents={props.agents}
            models={props.models}
            workers={[]}
            agentId={agentPick?.[0] ?? activeAgent}
            // Held too while a message of this tab is on its way (from the
            // click on Send until its row is logged or it is refused): a
            // message that looks sent goes to the agent it was sent to.
            picking={!!agentPick?.[1] || sending.some((o) => !o.failed)}
            onAgentChange={(id) => {
              setPickErr(null);
              setAgentPick([id, true]);
              pageApi.setAgent(sessionId, id).then(
                () => setAgentPick((p) => p && [p[0], false]),
                (x) => {
                  setAgentPick(null);
                  setPickErr(x instanceof Error ? x.message : String(x));
                },
              );
            }}
            approval={
              // The owner's own agentic chat on a node that can change it;
              // a sub-agent's follows its parent, a routine's its routine.
              meta?.profile === "agentic" && !meta.parent_session && !meta.no_callbacks && node?.max_approval
                ? {
                    value: meta.approval_mode,
                    max: node.max_approval,
                    host: meta.exec_mode === "host",
                    onChange: (m) => pageApi.setApprovalMode(sessionId, m),
                  }
                : undefined
            }
            steps={
              // As the approval mode: the owner's own agentic chat, on a
              // node that can change it (X.14).
              meta?.profile === "agentic" && !meta.parent_session && !meta.no_callbacks && node?.max_steps
                ? {
                    // The limit in force: the chat's, within the node's now.
                    value: Math.min(view?.max_steps ?? node.max_steps, node.max_steps),
                    max: node.max_steps,
                    onChange: (n) => pageApi.setStepLimit(sessionId, n),
                  }
                : undefined
            }
            // `@path` mentions and their completion (T63): a chat with a
            // folder that this person may write to.
            filesOf={meta?.profile === "agentic" && !blocked ? sessionId : undefined}
            agentState={running ? undefined : face}
            sessionModel={meta?.model}
            agenticBlocked={null}
            readOnly={blocked}
            running={running}
            choosing={!!holding[0]}
            choiceBox={choiceBox}
            modeNote={t("approval.modeNote")}
            // While a /compact runs, Stop cancels it (the same route).
            compacting={summarise.compacting && !running}
            link={live.state}
            draft={keepsDraft ? sessionId : undefined}
            voice={props.voice}
            onStop={() => void pageApi.cancel(sessionId)}
            compact={summarise.start}
            images={features?.media.vision_images ? { workspace: workspaceId, left: CHAT_IMAGES - imagesUsed } : null}
            handle={box}
            onText={onBoxText}
            held={held.map((o) => o.text).join("\n")}
            onSubmit={(s) => send(s)}
          />
          )}
        </div>
      </div>
        </div>
        {diffOf?.session === sessionId &&
          (() => {
            const cp = own(view?.checkpoints, diffOf.turn);
            const at = items.findIndex((x) => x.type === "turn_end" && x.turn_id === diffOf.turn);
            const laterChanges = items.slice(at + 1).some((x) => {
              if (x.type !== "turn_end") return false;
              const c = own(view?.checkpoints, x.turn_id);
              return !!c && c.files.length > 0 && !c.restored;
            });
            return (
              <Suspense fallback={null}>
                <DiffPanel
                  key={`${sessionId}:${diffOf.turn}`}
                  sessionId={sessionId}
                  turnId={diffOf.turn}
                  rewind={diffOf.rewind}
                  laterChanges={laterChanges}
                  headMove={cp?.head_moved}
                  laterMoves={diffOf.rewind && view ? laterMoves(view, at) : []}
                  canUndo={!blocked && (diffOf.rewind ? laterChanges || !!cp?.files.length : !!cp && !cp.restored && cp.files.length > 0)}
                  running={running}
                  onRewind={(rewind) => setDiffOf({ ...diffOf, rewind })}
                  onClose={closeDiff}
                />
              </Suspense>
            );
          })()}
        {sideFor.current === sessionId && (
          <Suspense fallback={null}>
            <SidePanel
              key={sessionId}
              sessionId={sessionId}
              view={view ?? null}
              models={props.models}
              away={!!node && !node.connected}
              nodeLabel={node ? (node.embedded ? "local" : node.hostname) : nodeId}
              yielded={diffOf?.session === sessionId}
              onReclaim={() => setDiffOf(null)}
            />
          </Suspense>
        )}
      </div>
    </section>
  );
  // The publish cards, saved-file items and "Save as artifact" share one lookup.
  return (
    <LiveCtx.Provider value={live}>
      <ChatArtifactsCtx.Provider value={artifacts}>{page}</ChatArtifactsCtx.Provider>
    </LiveCtx.Provider>
  );
}

// An open session: header, timeline, reply composer. Loaded lazily so the
// new-tab home screen stays small.
import "../lib/i18nPages";
import { Download, FileText, Package, Trash2 } from "lucide-react";
import { lazy, Suspense, useEffect, useLayoutEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { api } from "../api";
import { pageApi } from "../apiPages";
import { endWords, turnStart } from "../lib/callback";
import { dropDraft } from "../lib/drafts";
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
import { showHidden, shownName } from "../md/hidden";
import type { History } from "../bg/topics";
import type { Agent, Item, ModelInfo, NodeInfo, RoutineMark, SessionRow } from "../types";
import { inputBlock, inputState, progressSignature, progressSlots, type ProgressSlots } from "../ui/blocks";
import { Composer } from "./Composer";
import { AgentMark, agentAlert } from "./AgentAvatar";
import { ChatArtifactsCtx, SaveAsArtifact, SavedItem, stagedKeys, useChatArtifacts } from "./ChatArtifacts";
import { BubbleImages } from "./ChatImages";
import { ChatTitle } from "./ChatTitle";
import { DeleteChat } from "./DeleteChat";
import { useAvatarState } from "./avatarState";
import { Markdown } from "./Markdown";
import { NodeBadge } from "./NodeBadge";
import { ExecBadge } from "./ExecBadge";
import { repliedBlock, ReplyBubble } from "./ReplyBubble";
import { ToolCard, WebGroupCard } from "./ToolCard";
import { canJoinProject, ProjectChip } from "./ProjectChip";
import type { ProjectSummary } from "../lib/projects";

// Opt-in, and only its own small chunk: most people never load it.
const ChatGutter = lazy(() => import("../bg/ChatGutter"));
// Loaded on the chat's first running background command (T.8d step 2).
const BackgroundStrip = lazy(() => import("./BackgroundStrip"));
// Export as Markdown, loaded on its first click.
const ExportChat = lazy(() => import("./ExportChat"));
// Loaded on the chat's first task list (the agent's `todo` tool).
const TodoPanel = lazy(() => import("./TodoPanel"));

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
}) {
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [lastRewind, setLastRewind] = useState(false);
  const run = async (force: boolean, rewind = lastRewind) => {
    setBusy(true);
    setErr(null);
    setLastRewind(rewind);
    try {
      await pageApi.restoreTurn(props.sessionId, props.turnId, force, rewind);
    } catch (x) {
      setErr(x instanceof Error ? x.message : String(x));
    } finally {
      setBusy(false);
    }
  };
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
      {!props.restored && !props.blocked && (
        <button type="button" className="link" disabled={busy || props.running} onClick={() => void run(false, false)}>
          Undo file changes
        </button>
      )}
      {props.laterChanges && !props.blocked && (
        <button
          type="button"
          className="link"
          disabled={busy || props.running}
          title="Undo this turn and every later turn's file changes"
          onClick={() => void run(false, true)}
        >
          Rewind files to before this turn
        </button>
      )}
      {err && (
        <div className="error-text">
          {err}{" "}
          {err.includes("changed after the turn") && (
            <button type="button" className="link" onClick={() => void run(true)}>
              Undo anyway
            </button>
          )}
        </div>
      )}
    </div>
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
  const scroller = useRef<HTMLDivElement>(null);
  const stick = useRef(true);

  useLayoutEffect(() => {
    const el = scroller.current;
    if (el && stick.current) el.scrollTop = el.scrollHeight;
  });

  const meta = view?.meta;
  const running = !!view?.running_turn;
  const items = view?.items ?? [];
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
  const activeAgent = view?.active_agent ?? props.row?.agent_id ?? "general";
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
  const answering = view?.running_agent?.agent_id ?? activeAgent;
  const facing = props.agents.find((a) => a.id === answering);
  const sessionAgent = props.agents.find((a) => a.id === activeAgent);
  const notes = fallbackNotes(items);
  const routine = props.row?.routine;
  useRunSeen(routine);
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
  const stripFor = useRef<string | null>(null);
  if (view?.background && Object.values(view.background).some((b) => !b.ended)) stripFor.current = sessionId;
  const todoFor = useRef<string | null>(null);
  if (view?.todo?.length) todoFor.current = sessionId;

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
              {agentName(activeAgent)}
            </span>
            <span className="muted">{meta?.model}</span>
            {agentic && <NodeBadge node={node} nodeId={nodeId} />}
            {agentic && <ExecBadge mode={meta?.exec_mode} node={node} />}
            {agentic && <span className="mono muted ellipsis">{shownName(meta?.cwd ?? "")}</span>}
            {view && view.usage.input_tokens + view.usage.output_tokens > 0 && (
              <span
                className="muted meter"
                title={`${view.usage.input_tokens.toLocaleString()} in · ${view.usage.output_tokens.toLocaleString()} out${
                  view.usage.cache_read_tokens
                    ? ` · ${view.usage.cache_read_tokens.toLocaleString()} of the input read from the provider's cache`
                    : ""
                }`}
              >
                {compact(view.usage.input_tokens + view.usage.output_tokens)} {t("conv.tokens")} ·{" "}
                {compact(view.context_tokens)} {t("conv.context")}
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

      <div
        className="timeline"
        ref={scroller}
        onScroll={() => {
          const el = scroller.current;
          if (el) stick.current = el.scrollHeight - el.scrollTop - el.clientHeight < 80;
        }}
      >
        <BlockCtx slots={slots} inputs={inputs}>
          <div className="column">
            {!view || view.head_seq === 0 ? (
              <div className="muted pad">Loading…</div>
            ) : (
              view.items.map((it, i) => {
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
                    view.variant_selection[g] ?? answers.find((a) => a.variant?.index === 0)?.message_id;
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
                          <div className="bubble">{it.text}</div>
                        )}
                        {it.attachments && (
                          <ul className="chips">
                            {it.attachments.map((n, j) => (
                              <li key={j} className="chip">
                                <FileText size={14} /> {n}
                              </li>
                            ))}
                          </ul>
                        )}
                        {!!it.images?.length && <BubbleImages images={it.images} />}
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
                        {final && <CopyMessage text={it.text} />}
                      </div>
                    );
                  }
                  case "fallback":
                    return notes.attached.has(i) ? null : <FallbackNote key={i} note={it} />;
                  case "tool":
                    return (
                      <ToolCard key={i} sessionId={sessionId} item={it} blocked={!!blocked} projectId={props.row?.project_id} cwd={meta?.cwd} />
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
                      <div key={i} className="msg error">
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
                          {it.bg_id} ended ({it.reason.replace(/_/g, " ")}
                          {it.exit_code != null ? `, code ${it.exit_code}` : ""}) after {Math.round(it.ran_ms / 1000)} s
                        </summary>
                        {it.tail && <pre>{showHidden(it.tail)}</pre>}
                      </details>
                    );
                  case "agent_changed":
                    return (
                      <div key={i} className="divider">
                        {it.agent.name}
                      </div>
                    );
                  case "summary":
                    // Only the latest summary is what the model sees now.
                    return view.items.findLastIndex((x) => x.type === "summary") !== i ? null : (
                      <details key={i} className="divider summary">
                        <summary>{t("conv.summary")}</summary>
                        <p>{it.text}</p>
                      </details>
                    );
                  case "turn_end": {
                    const cp = view.checkpoints[it.turn_id];
                    const laterChanges = view.items.slice(i + 1).some((x) => {
                      if (x.type !== "turn_end") return false;
                      const c = view.checkpoints[x.turn_id];
                      return !!c && c.files.length > 0 && !c.restored;
                    });
                    const undo =
                      // Only a rewind would show for a turn that changed nothing.
                      cp && cp.after && (cp.files.length > 0 || (laterChanges && !blocked)) ? (
                        <Undo
                          key={`u${i}`}
                          sessionId={sessionId}
                          turnId={it.turn_id}
                          files={cp.files}
                          restored={cp.restored || cp.files.length === 0}
                          running={running}
                          laterChanges={laterChanges}
                          blocked={!!blocked}
                        />
                      ) : null;
                    return it.reason === "completed" ? (
                      undo
                    ) : (
                      <div key={i}>
                        <div className={`divider r-${it.reason}`}>turn {it.reason}</div>
                        {undo}
                      </div>
                    );
                  }
                }
              })
            )}
            {running && view?.running_agent && (
              <div className="thinking">
                {facing && <AgentMark agent={facing} state={face} size={22} />}
                {agentName(view.running_agent.agent_id)}{" "}
                {face === "unsure" ? "is waiting for you" : face === "working" ? "is working…" : "is thinking…"}
              </div>
            )}
          </div>
        </BlockCtx>
      </div>

      <div className="bottom">
        <div className="column">
          {todoFor.current === sessionId && (
            <Suspense fallback={null}>
              <TodoPanel key={sessionId} items={view?.todo ?? []} running={running} />
            </Suspense>
          )}
          {stripFor.current === sessionId && (
            <Suspense fallback={null}>
              <BackgroundStrip
                key={sessionId}
                sessionId={sessionId}
                view={view ?? null}
                away={!!node && !node.connected}
                nodeLabel={node ? (node.embedded ? "local" : node.hostname) : nodeId}
              />
            </Suspense>
          )}
          <Composer
            where="session"
            agents={props.agents}
            models={props.models}
            workers={[]}
            agentId={activeAgent}
            onAgentChange={(id) => void pageApi.setAgent(sessionId, id)}
            agentState={running ? undefined : face}
            sessionModel={meta?.model}
            agenticBlocked={null}
            readOnly={blocked}
            running={running}
            draft={keepsDraft ? sessionId : undefined}
            voice={props.voice}
            onStop={() => void pageApi.cancel(sessionId)}
            images={features?.media.vision_images ? { workspace: workspaceId, left: CHAT_IMAGES - imagesUsed } : null}
            onSubmit={async (s) => {
              stick.current = true;
              await api.prompt(sessionId, s.text, {
                ...(s.effort ? { effort: s.effort } : {}),
                ...(s.attachments.length ? { attachments: s.attachments } : {}),
                ...(s.images?.length ? { images: s.images } : {}),
              });
            }}
          />
        </div>
      </div>
    </section>
  );
  // The publish cards, saved-file items and "Save as artifact" share one lookup.
  return <ChatArtifactsCtx.Provider value={artifacts}>{page}</ChatArtifactsCtx.Provider>;
}

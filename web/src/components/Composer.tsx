import { ArrowUp, AtSign, CornerDownRight, FileText, Globe, Image, Loader2, Mic, Paperclip, Square, X } from "lucide-react";
import { type Dispatch, Fragment, lazy, type SetStateAction, Suspense, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { t } from "../lib/i18n";
import { isTag, marked, mentionable, pickable } from "../lib/agents";
import { activeMention, filterAgents, matchedAlias, insertMention, isPathToken, leadingMention } from "../lib/mentions";
import { cycleMode, enabledModes, MODES, type Mode } from "../lib/modes";
import { modelLabel, offeredModels, rawModel, resolveModel } from "../lib/models";
import { radioKeys, radioTab } from "../lib/radios";
import { api } from "../api";
import { readDraft, writeDraft } from "../lib/drafts";
import { bytes } from "../lib/bytes";
import { parseCompact } from "../lib/compactCommand";
import type { Picture } from "../lib/downscale";
import { abilities, allEngines, type CustomEngine, engineByUrl, engineMention, engineOrigin, type SearchKind, supports } from "../lib/engines";
import { EngineIcon } from "./EngineIcon";
import { Select } from "./Select";
import { AgentMark, agentAlert } from "./AgentAvatar";
import type { ConnState } from "../live";
import type { Agent, ApprovalMode, AvatarState, Effort, ExecMode, ModelInfo, NodeInfo, UploadedAttachment } from "../types";
import { type HostRight, mayPickHost } from "../lib/execModes";
import { shownName } from "../md/hiddenChars";
import type { ProjectChoice } from "./ProjectPicker";

// An attached image's chip loads through a link (P-1); its own chunk, so
// the first load does not carry the link code.
const LinkedImg = lazy(() => import("./Linked"));
// The host shell's choice: only those who may pick it load it (N.10a).
const ExecChoice = lazy(() => import("./ExecChoice"));
// The approval mode's picker (T50): only for agentic chats on a node that
// announced its modes.
const ApprovalModePick = lazy(() => import("./ApprovalModePick"));
// The chat's step limit (X.14), beside the approval mode: same chats.
const StepLimitPick = lazy(() => import("./StepLimitPick"));
const ExecSwitch = lazy(() => import("./ExecSwitch"));
// Where a new agentic chat works (X.26): the Project button and its popup.
const ProjectPicker = lazy(() => import("./ProjectPicker"));

const EFFORTS: Effort[] = ["low", "medium", "high", "max"];


export interface Submission {
  text: string;
  mode: Mode;
  agentId: string;
  model: string;
  effort: Effort | null;
  nodeId: string;
  cwd: string;
  /** Home, Ask mode: a temporary chat. */
  incognito: boolean;
  attachments: string[];
  /** Home, Ask: also answer with this model, side by side. */
  compareWith: string | null;
  /** Search mode: the engine URL template and the query (an @engine removed). */
  search?: { url: string; query: string };
  /** Media item ids of images sent with the message. */
  images?: string[];
  /** Agentic: how its commands run (N.10a); `host` only once its warning was confirmed. */
  execMode?: ExecMode;
  /** Home, agentic: the project picked in the Project popup (X.26). */
  projectId?: string;
  /** Agentic: the approval mode the person picked (T50); absent: the node's default. */
  approvalMode?: ApprovalMode;
  /** Session, a chat with a folder: the paths its text mentions with `@path` (T63). */
  files?: string[];
  /** The box's attachments and images as they were, to put back (lib/outbox). */
  uploads?: UploadedAttachment[];
  pictures?: Picture[];
}

/** What the chat may do to the box: put a refused message back, take a
 * retried one out (Conversation, lib/outbox). */
export interface ComposerHandle {
  setText: Dispatch<SetStateAction<string>>;
  setFiles: Dispatch<SetStateAction<UploadedAttachment[]>>;
  setPictures: Dispatch<SetStateAction<Picture[]>>;
  /** Nothing typed, attached or picked (as of the last render). */
  empty: () => boolean;
  /** What the box holds now (as of the last render): its text, the keys of
   * its pictures, and how many attachments. */
  content: () => { text: string; pictures: string[]; files: number };
  focus: () => void;
}

/** Files and images together in one message (the server counts the same). */
const MAX_ATTACHMENTS = 10;
/** Images per message (docs/media.md §6.6). */
const MAX_IMAGES = 4;
const isImage = (f: File) => f.type.startsWith("image/") || /\.hei[cf]$/i.test(f.name);

export function Composer(props: {
  /** `home` shows the mode switch and model picker; `session` is a reply box. */
  where: "home" | "session";
  agents: Agent[];
  models: ModelInfo[];
  workers: NodeInfo[];
  /** Selected agent (home: the choice for the new chat; session: the session's agent). */
  agentId: string;
  onAgentChange: (id: string) => void;
  /** Session: a pick waits for the server's answer (the picker is held). */
  picking?: boolean;
  /** Session: what the selected agent is doing, for its avatar on the pill. */
  agentState?: AvatarState;
  /** Session model (session view), for the effort control. */
  sessionModel?: string;
  defaultModel?: string | null;
  agenticBlocked: string | null;
  /** Session: why this chat cannot be driven here (its workspace role, the
   * account's grant); the box is read-only and says so. Stop still works. */
  readOnly?: string | null;
  running?: boolean;
  /** The chat whose unsent text this box keeps (lib/drafts); none: kept nowhere. */
  draft?: string;
  /** Messages of this chat not in its log and not in the box (refused ones):
   * the draft keeps them ahead of the box's text. */
  held?: string;
  handle?: { current: ComposerHandle | null };
  /** The box's text changed (and once when it opens): the chat decides what
   * to offer from it (a rewound message already in the box). */
  onText?: (text: string) => void;
  onSubmit: (s: Submission) => Promise<void>;
  onStop?: () => void;
  /** Home: the classifier's guess for this text (null = no opinion). */
  predict?: (text: string) => Mode | null;
  /** Home: called after a successful submit with the mode actually used. */
  onLearn?: (text: string, mode: Mode) => void;
  /** Home: text/mode changes, for the similar-chats panel. */
  onTextChange?: (text: string, mode: Mode) => void;
  /** Home: the mode changed, picked by the user (a click, Tab) or by the bar (the classifier, a reset). */
  onModeChange?: (mode: Mode, by: "user" | "auto") => void;
  /** Home: ArrowDown at the end of the text; return true if handled. */
  onArrowDown?: () => boolean;
  /** The server can transcribe dictation. */
  voice?: boolean;
  /** Search mode: the user's default engines and their own engines. */
  search?: { text: string; image: string; custom?: CustomEngine[] };
  /** Home bar on a project's page: no search or incognito; the project's node and folder first. */
  project?: { nodeId: string | null; cwd: string | null };
  /** Images may go with the message (the server takes chat images): they
   * upload to `workspace`'s Media. `pending`: pick up "Use in chat" (the
   * home screen). `left`: how many more this chat takes. */
  images?: { workspace: string | null; pending?: boolean; left?: number } | null;
  /** Home: the workspace whose projects the Project popup lists. */
  workspace?: string | null;
  /** Home: who may pick the host shell (N.10a); absent, nobody here. */
  hostRight?: HostRight;
  /** Session: the chat's approval mode and how to change it (T50); absent:
   * not changeable here (a chat, a sub-agent's, a routine's, an old node). */
  approval?: { value: ApprovalMode; max: ApprovalMode; host: boolean; onChange: (m: ApprovalMode) => Promise<unknown> };
  /** Session: the chat's execution mode and how to switch it (N.10b); absent:
   * not switchable here (a chat, a sub-agent's, a routine's, an old node). */
  exec?: {
    value?: ExecMode;
    offered: ExecMode[];
    mayHost: boolean;
    node: string;
    busy: string | null;
    onChange: (m: ExecMode) => Promise<unknown>;
  };
  /** Session: the chat's step limit and the node's maximum (X.14); absent:
   * not changeable here. */
  steps?: { value: number; max: number; onChange: (n: number) => Promise<unknown> };
  /** Session, the owner's chat with a folder: its id, for `@path`
   * completion and mentions (T63). Absent: no files are offered. */
  filesOf?: string;
  /** Session: this chat's id, for a file a chat without a folder keeps for the agent to read (T95). */
  chatId?: string;
  /** Session: `/compact [focus]` summarises the chat now instead of sending a message (X.20). */
  compact?: (focus: string) => Promise<unknown>;
  /** Session: a call waits for approval and its choice is shown in the
   * field's place (row W.6): the field is hidden, its draft and caret kept. */
  choosing?: boolean;
  /** Session: the choice's own container, which takes the focus the field
   * had (or nobody had) when the choice appears. */
  choiceBox?: React.RefObject<HTMLElement | null>;
  /** Session: said next to the mode picker while a choice is up. */
  modeNote?: string;
  /** Session: a /compact runs; Stop cancels it. */
  compacting?: boolean;
  /** The page's live connection, in a chat. */
  link?: ConnState;
  /** Session: the chat's figures (tokens, context, cost, quota), shown at
   * the right of the strip under the box. The chat draws them; the home
   * screen has no session and passes none. */
  figures?: React.ReactNode;
}) {
  const home = props.where === "home";
  // The reason the commands control is disabled is shown once it is asked for.
  const [execNote, setExecNote] = useState(false);
  const execBusy = props.exec?.busy ?? null;
  useEffect(() => {
    if (!execBusy) setExecNote(false);
  }, [execBusy]);
  const [text, setText] = useState(() => (props.draft ? readDraft(props.draft) : ""));
  const [mode, setMode] = useState<Mode>("ask");
  // Once the user picks a mode for this message, the classifier stays quiet.
  const [manual, setManual] = useState(false);
  // Who caused the next mode change; read once by the effect that reports it.
  const pickedBy = useRef<"user" | "auto">("auto");
  const pickMode = (m: Mode) => {
    setManual(true);
    if (m === mode) return;
    pickedBy.current = "user";
    setMode(m);
  };
  // The model: the user's pick in this bar, then the agent's default, the
  // user's default, the server's. A pick made under another agent gives way
  // to this agent's default. General's default is the server's, which the
  // user's own outranks. A leading @agent answers this message, on its
  // default. Models the provider no longer lists are not offered, though a
  // session or default naming one still resolves to it.
  const offered = offeredModels(props.models);
  const [picked, setPicked] = useState<{ agent: string; model: string } | null>(null);
  const override = leadingMention(
    text,
    props.agents.map((a) => a.id),
  );
  const answering = override ?? props.agentId;
  const own = props.agents.find((a) => a.id === answering);
  const agentDefault = own && own.id !== "general" ? resolveModel(props.models, own.default_model)?.id : undefined;
  const model =
    (picked && (picked.agent === answering || !agentDefault) && props.models.some((m) => m.id === picked.model)
      ? picked.model
      : undefined) ??
    agentDefault ??
    resolveModel(props.models, props.defaultModel)?.id ??
    offered.find((m) => m.default)?.id ??
    offered[0]?.id ??
    "";
  const setModel = (m: string) => setPicked({ agent: answering, model: m });
  const [effort, setEffort] = useState<Effort | null>(null);
  const [nodeId, setNodeId] = useState(props.project?.nodeId ?? props.workers[0]?.node_id ?? "");
  // Where the new chat works (X.26): a folder, and the project picked in
  // the Project popup with the node it is on. Without a project page's
  // folder, the Project button starts at the latest folder used.
  const [place, setPlace] = useState<Partial<ProjectChoice>>({ cwd: props.project?.cwd ?? "" });
  const cwd = place.cwd ?? "";
  const project = place.project;
  // A project is on its node: another node starts with none. A folder
  // (no node named) stays, as the typed path did.
  const unpick = () => place.nodeId && setPlace({});
  // The new chat's mode: `host` only after its warning (ExecChoice).
  // Bound to the node it was confirmed for (review L-j): a node switch,
  // the user's or an automatic one, never carries it to another node.
  const [hostFor, setHostFor] = useState<string | null>(null);
  const worker = props.workers.find((w) => w.node_id === nodeId);
  const hostPickable = mayPickHost(worker, props.hostRight);
  // The new chat's approval mode, as picked, for one node (like `hostFor`).
  const [approvalFor, setApprovalFor] = useState<{ node: string; mode: ApprovalMode } | null>(null);
  const approvalPick = approvalFor?.node === nodeId ? approvalFor.mode : undefined;
  const [caret, setCaret] = useState(0);
  const [pick, setPick] = useState(0);
  const [popupClosed, setPopupClosed] = useState(false);
  // While a message is sent: its text.
  const [busy, setBusy] = useState<string | boolean>(false);
  const [incognito, setIncognito] = useState(false);
  const [compareWith, setCompareWith] = useState<string | null>(null);
  const [searchKind, setSearchKind] = useState<SearchKind>("text");
  const [enginePick, setEnginePick] = useState<Partial<Record<SearchKind, string>>>({});
  const [files, setFiles] = useState<UploadedAttachment[]>([]);
  const [uploading, setUploading] = useState(0);
  const fileInput = useRef<HTMLInputElement>(null);
  const [pictures, setPictures] = useState<Picture[]>([]);
  if (props.handle) props.handle.current = { setText, setFiles, setPictures, empty: () => !text.trim() && !files.length && !pictures.length, content: () => ({ text, pictures: pictures.map((p) => p.key), files: files.length }), focus: () => ta.current?.focus() };
  const onText = props.onText;
  useEffect(() => onText?.(text), [text]);
  const [rec, setRec] = useState<"idle" | "recording" | "transcribing">("idle");
  const recorder = useRef<MediaRecorder | null>(null);

  const toggleMic = async () => {
    if (rec === "recording") {
      recorder.current?.stop();
      return;
    }
    setErr(null);
    try {
      // A chunk that does not load (an update since, the network): said
      // like the microphone's other errors.
      const { record } = await import("../lib/voice").catch(() => {
        throw Error("could not load; reload the page");
      });
      recorder.current = await record({
        transcribing: () => setRec("transcribing"),
        done: (said, error) => {
          if (said) setText((t) => (t.trim() ? `${t.trimEnd()} ${said}` : said));
          if (error) setErr(error);
          else ta.current?.focus();
          setRec("idle");
        },
      });
      setRec("recording");
    } catch (x) {
      setErr(`microphone: ${x instanceof Error ? x.message : String(x)}`);
    }
  };
  const [err, setErr] = useState<string | null>(null);
  // "The server is not reachable" is true only until the live connection is
  // back (a chat passes its connection; the home screen has none to follow).
  const link = props.link;
  useEffect(() => {
    if (link === "live") setErr((e) => (e === t("error.unreachable") ? null : e));
  }, [link]);
  const ta = useRef<HTMLTextAreaElement>(null);
  // Caret to restore right after React commits a programmatic text change,
  // before the next keystroke can land in the wrong place.
  const pendingCaret = useRef<number | null>(null);
  useLayoutEffect(() => {
    const c = pendingCaret.current;
    if (c !== null && ta.current) {
      ta.current.setSelectionRange(c, c);
      pendingCaret.current = null;
    }
  }, [text]);

  // The mode switch's thumb sits under the chosen button: its offset and
  // width go into --x and --w. The suffix "· auto" and the phone layout
  // change the widths, hence the observer. `.ready` (which turns the slide
  // on) waits for the first placement, so the first paint does not slide.
  const track = useRef<HTMLDivElement>(null);
  const [thumbReady, setThumbReady] = useState(false);
  const auto = !manual && !!text.trim();
  const placeThumb = () => {
    const on = track.current?.querySelector<HTMLElement>("button.on");
    if (!track.current || !on) return false;
    track.current.style.setProperty("--x", `${on.offsetLeft}px`);
    track.current.style.setProperty("--w", `${on.offsetWidth}px`);
    return true;
  };
  useLayoutEffect(() => {
    // Reading a size applies the new position before .ready can animate it.
    if (placeThumb() && !thumbReady && track.current!.offsetWidth) setThumbReady(true);
  }, [home, mode, auto, thumbReady]);
  useEffect(() => {
    const el = track.current;
    if (!el || typeof ResizeObserver !== "function") return;
    const ro = new ResizeObserver(() => placeThumb());
    ro.observe(el);
    return () => ro.disconnect();
  }, [home]);

  // Keep selections valid as lists load.
  useEffect(() => {
    if (!props.workers.some((w) => w.node_id === nodeId)) {
      setNodeId(props.workers[0]?.node_id ?? "");
      unpick();
    }
  }, [props.workers, nodeId]);
  useEffect(() => {
    if (mode === "agentic" && props.agenticBlocked) setMode("ask");
  }, [mode, props.agenticBlocked]);
  const { predict, onTextChange, onModeChange } = props;
  useEffect(() => {
    if (!home || manual || !predict) return;
    const guess = text.trim() ? predict(text) : "ask";
    if (guess) setMode(guess);
  }, [home, manual, predict, text]);
  useEffect(() => {
    onTextChange?.(text, mode);
  }, [onTextChange, text, mode]);
  useEffect(() => {
    onModeChange?.(mode, pickedBy.current);
    pickedBy.current = "auto";
  }, [onModeChange, mode]);
  // The chat may be known to keep drafts only once its log arrives (not
  // incognito): then its draft shows, unless something is typed already.
  // Writes follow the text alone, so the key's arrival erases nothing.
  const draft = useRef(props.draft);
  useEffect(() => {
    const k = (draft.current = props.draft);
    if (k) setText((t) => t || readDraft(k));
  }, [props.draft]);
  // Until the server has it, the message being sent stays in the draft
  // ahead of what is typed since, as do refused ones (`held`): a reload or a
  // closed chat loses none of them.
  useEffect(() => {
    if (draft.current)
      writeDraft(draft.current, [busy !== true && busy, props.held, text].filter((x) => x && !(x != text && text.includes(x))).join("\n"));
  }, [text, busy, props.held]);
  useEffect(() => {
    ta.current?.focus();
  }, []);
  // The choice takes the field's place: whoever was typing is taken out of
  // it (so no key lands in a hidden field) and put back, caret kept, when
  // the call is answered; anyone elsewhere keeps their focus.
  const typing = useRef(false);
  useLayoutEffect(() => {
    const el = ta.current;
    if (props.choosing) {
      const at = document.activeElement;
      typing.current = at === el;
      if (typing.current) el!.blur();
      // Focus waits on the choice's container (never a button): the rest of
      // a typing burst lands there and is dropped, not in the page.
      if (typing.current || at === document.body) props.choiceBox?.current?.focus({ preventScroll: true });
    } else {
      // Focus on nothing (the page) is where the choice's own button left it
      // when it went; a second call asked after the first leaves no memory of
      // the typing in between (the browser drops focus from a hidden field).
      const lost = typing.current || document.activeElement === document.body;
      typing.current = false;
      if (lost && !props.readOnly) el?.focus();
    }
  }, [props.choosing]);

  const inProject = !!props.project;
  const enabled = useMemo(
    () => enabledModes(props.agenticBlocked).filter((m) => !inProject || m !== "search"),
    [props.agenticBlocked, inProject],
  );
  // Search mode: which engine runs. A leading @engine wins (and switches to
  // the kind it can do), then the selector, then the user's default.
  const engines = useMemo(() => allEngines(props.search?.custom), [props.search?.custom]);
  const searching = home && mode === "search";
  const typedEngine = searching ? engineMention(text, engines) : null;
  let kind: SearchKind = searchKind;
  if (typedEngine && !supports(typedEngine.engine, kind)) kind = kind === "text" ? "image" : "text";
  const defaultUrl = (kind === "text" ? props.search?.text : props.search?.image) ?? "";
  const engine =
    typedEngine?.engine ??
    engines.find((e) => e.id === enginePick[kind] && supports(e, kind)) ??
    engineByUrl(engines, defaultUrl, kind);
  const engineUrl = (engine ? (kind === "text" ? engine.text : engine.image) : undefined) ?? defaultUrl;

  const filesOf = searching ? undefined : props.filesOf;
  const mention = popupClosed ? null : activeMention(text, caret, !!filesOf);
  // A token with `/` or `.` names a file of the chat's folder (agents
  // first otherwise): the node lists matches, asked once typing pauses.
  const fileQuery = mention && filesOf && isPathToken(mention.query) ? mention.query : null;
  const [fileHits, setFileHits] = useState<{ query: string; paths: string[] } | null>(null);
  useEffect(() => {
    if (fileQuery == null || !filesOf) return;
    let live = true;
    const timer = setTimeout(() => {
      const done = (paths: string[]) => live && setFileHits({ query: fileQuery, paths });
      // Lazy: only a chat with a folder loads the file part.
      import("../lib/fileMentions")
        .then((m) => m.listFiles(filesOf, fileQuery))
        .then(done, () => done([]));
    }, 150);
    return () => {
      live = false;
      clearTimeout(timer);
    };
  }, [fileQuery, filesOf]);
  const hits = fileQuery != null && fileHits?.query === fileQuery ? fileHits.paths : null;
  // The paths the text mentions that the folder holds, shown as chips
  // before sending (each can be taken off): only these go with the
  // message, so nothing reaches the model the person did not see listed.
  const [found, setFound] = useState<string[]>([]);
  const [dropped, setDropped] = useState<string[]>([]);
  const known = useRef(new Map<string, { ok: boolean; at: number }>());
  const hasPathToken = !!filesOf && /(^|[\s([{'"])@(?:"[^"\n@]+"|[^\s@]*[/.])/.test(text);
  useEffect(() => {
    if (!filesOf || !hasPathToken) {
      setFound([]);
      return;
    }
    let live = true;
    const timer = setTimeout(() => {
      import("../lib/fileMentions")
        .then((m) => m.existing(filesOf, m.fileMentions(text), known.current))
        .then((paths) => live && setFound(paths), () => {});
    }, 300);
    return () => {
      live = false;
      clearTimeout(timer);
    };
  }, [text, filesOf, hasPathToken]);
  const attached = found.filter((p) => !dropped.includes(p));
  // The agents this text calls, when it calls several (X.37): the rule is
  // loaded when the text holds an `@` (lib/calledNow.ts).
  const [mentions, setMentions] = useState<{ called: string[]; tags: string[]; over: boolean }>({ called: [], tags: [], over: false });
  useEffect(() => {
    if (searching || !text.includes("@")) {
      setMentions((c) => (c.called.length || c.tags.length || c.over ? { called: [], tags: [], over: false } : c));
      return;
    }
    let live = true;
    import("../lib/calledNow")
      .then((m) => m.calledIn(text, props.agents))
      .then((c) => live && setMentions(c), () => {});
    return () => {
      live = false;
    };
  }, [text, props.agents, searching]);
  const { called, tags } = mentions;
  const several = called.length > 1;
  // On send, the check of the text as sent (a token typed within the
  // debounce, a file made since): waited for at most 2 s, else the chips
  // shown. Never a path the person took off.
  const sendable = async (id: string, body: string): Promise<string[]> => {
    const m = await import("../lib/fileMentions");
    const checked = await Promise.race([
      m.existing(id, m.fileMentions(body), known.current),
      new Promise<null>((r) => setTimeout(() => r(null), 2000)),
    ]);
    return (checked ?? attached).filter((p) => !dropped.includes(p));
  };
  const suggestions: { id: string; title: string; hint: string; origin?: string | null; agent?: Agent; file?: boolean }[] = !mention
    ? []
    : fileQuery != null
      ? (hits ?? []).map((p) => ({ id: p, title: p, hint: "", file: true }))
      : searching
      ? engines
          .filter((e) => e.id.startsWith(mention.query.toLowerCase()) || e.name.toLowerCase().startsWith(mention.query.toLowerCase()))
          .slice(0, 8)
          .map((e) => ({ id: e.id, title: e.name, hint: abilities(e), origin: engineOrigin(e) }))
      : filterAgents(mentionable(props.agents), mention.query).map((a) => ({
          id: a.id,
          title: a.id,
          hint: [isTag(a) && t("composer.tagHint"), matchedAlias(a, mention.query) && `@${matchedAlias(a, mention.query)}`, marked(a, a.description || a.name)].filter(Boolean).join(" · "),
          agent: a,
        }));
  const popupOpen = suggestions.length > 0;
  const pickedAgent = props.agents.find((a) => a.id === props.agentId);
  const pickedAlert = pickedAgent ? agentAlert(pickedAgent) : null;
  // A chat's agent changes between turns only (the node refuses it otherwise).
  const stillWorking = !home && !!(props.running || props.compacting || props.picking);
  const effortModel = home ? model : props.sessionModel;
  const effortOk = resolveModel(props.models, effortModel)?.effort ?? false;
  // Images: not in search or a temporary chat; every model answering must
  // see them (one the list does not know is left to the server).
  const tempChat = home && mode === "ask" && incognito;
  const canImage = !!props.images && mode !== "search" && !tempChat;
  const blind = canImage
    ? [effortModel, home && mode === "ask" ? compareWith : null].map((m) => resolveModel(props.models, m)).find((m) => m && !m.vision)
    : undefined;
  const imageRoom = Math.min(MAX_IMAGES, props.images?.left ?? MAX_IMAGES, MAX_ATTACHMENTS - files.length) - pictures.length;
  const sendingImages = pictures.some((x) => !x.media_id);

  const choose = (id: string) => {
    if (!mention) return;
    const r = insertMention(text, mention.start, caret, id);
    pendingCaret.current = r.caret;
    setText(r.text);
    setCaret(r.caret);
    ta.current?.focus();
  };

  const submit = async () => {
    const body = text.trim();
    // While a turn runs a message still goes: the agent reads it at its
    // next step (X.5).
    if (!body || busy || uploading || sendingImages || props.readOnly) return;
    // `/compact [focus]`: a command to the chat's node, never a message.
    const asked = !home && props.compact ? parseCompact(body) : null;
    if (asked && "error" in asked) {
      setErr(asked.error);
      return;
    }
    if (asked && props.compact) {
      setBusy(true);
      setErr(null);
      try {
        await props.compact(asked.focus);
        setText("");
      } catch (x) {
        setErr(x instanceof Error ? x.message : String(x));
      } finally {
        setBusy(false);
      }
      return;
    }
    if (mode === "agentic" && (!cwd.trim() || !nodeId)) {
      setErr(t("composer.folder"));
      return;
    }
    // Always a mode the person chose: `host` only once confirmed, else
    // `sandbox`, which a node without it refuses (never the node's default).
    const exec: ExecMode = hostPickable && hostFor === nodeId ? "host" : "sandbox";
    if (mode === "agentic" && hostPickable && exec === "sandbox" && !worker?.exec_modes?.includes("sandbox")) {
      setErr("This node runs commands only in the host shell: choose it under Commands first.");
      return;
    }
    const images = pictures.map((x) => x.media_id!);
    if (images.length && (blind || tempChat)) {
      setErr(`${blind ? `${modelLabel(blind, props.models)} can't read images` : "A temporary chat takes no images"}; remove them to send.`);
      return;
    }
    // The text leaves the box at once, for the chat (or, from home, the line
    // above the box) to show as sent; a refusal puts it back.
    setBusy(body);
    setText("");
    setErr(null);
    try {
      await props.onSubmit({
        text: body,
        mode: home ? mode : "ask",
        agentId: props.agentId,
        model,
        effort: effortOk ? effort : null,
        nodeId,
        cwd: cwd.trim(),
        incognito: home && mode === "ask" && incognito,
        attachments: files.map((f) => f.attachment_id),
        compareWith: home && mode === "ask" && compareWith && compareWith !== model ? compareWith : null,
        search: searching ? { url: engineUrl, query: typedEngine ? typedEngine.query.trim() || body : body } : undefined,
        ...(images.length ? { images } : {}),
        uploads: files,
        pictures,
        ...(home && mode === "agentic" ? { execMode: exec } : {}),
        ...(home && mode === "agentic" && approvalPick ? { approvalMode: approvalPick } : {}),
        projectId: project?.id,
        ...(filesOf && hasPathToken ? { files: await sendable(filesOf, body) } : {}),
      });
      if (home) props.onLearn?.(body, mode);
      setFiles([]);
      setFound([]);
      setDropped([]);
      setPictures([]);
      setManual(false);
      // The host shell is chosen again, with its warning, for each chat.
      setHostFor(null);
      setApprovalFor(null);
    } catch (x) {
      setText((c) => (c.trim() ? `${body}\n${c}` : body));
      setErr(x instanceof Error ? x.message : String(x));
    } finally {
      setBusy(false);
    }
  };

  // A coding chat (or the home bar in agentic mode) takes any file: the
  // node saves what the model cannot read into the chat's folder (T94).
  const folder = home ? mode === "agentic" : !!props.filesOf;
  const addFiles = async (list: File[]) => {
    const picked = list.slice(0, Math.max(0, MAX_ATTACHMENTS - files.length - pictures.length));
    setErr(picked.length < list.length ? t("composer.tooManyFiles") : null);
    for (const f of picked) {
      setUploading((n) => n + 1);
      try {
        const up = await api.upload(f, { session: home ? undefined : (props.filesOf ?? props.chatId), folder: folder, agent: folder ? undefined : props.agentId || undefined });
        setFiles((prev) => [...prev, up]);
      } catch (x) {
        setErr(`${f.name}: ${x instanceof Error ? x.message : String(x)}`);
      } finally {
        setUploading((n) => n - 1);
      }
    }
  };

  // Images are made small in this browser and uploaded by lib/downscale,
  // loaded on the first one; each chip waits for its Media id.
  const addImages = (list: File[]) => {
    const at = props.images;
    if (!at || !list.length) return;
    const bar = { pictures, set: setPictures, fail: setErr, workspace: at.workspace, room: imageRoom };
    import("../lib/downscale").then((d) => d.addImages(list, bar), (x) => setErr(String(x)));
  };
  // The home bar takes what "Use in chat" left, once images are possible here.
  const pickUp = !!props.images?.pending;
  useEffect(() => {
    if (pickUp) import("../lib/downscale").then((d) => d.pickUp(setPictures), () => {});
  }, [pickUp]);
  // Pasted or dropped pictures go with the message; anything else keeps
  // what the browser does with it (pasted text stays text).
  const takeImages = (e: { preventDefault(): void }, files: FileList) => {
    const list = canImage ? [...files].filter(isImage) : [];
    if (!list.length) return;
    e.preventDefault();
    addImages(list);
  };

  const onKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.nativeEvent.isComposing) return;
    if (popupOpen) {
      if (e.key === "ArrowDown" || e.key === "ArrowUp") {
        e.preventDefault();
        const d = e.key === "ArrowDown" ? 1 : -1;
        setPick((p) => (p + d + suggestions.length) % suggestions.length);
        return;
      }
      if (e.key === "Enter" || e.key === "Tab") {
        e.preventDefault();
        choose(suggestions[Math.min(pick, suggestions.length - 1)]!.id);
        return;
      }
      if (e.key === "Escape") {
        e.preventDefault();
        setPopupClosed(true);
        return;
      }
    }
    if (e.key === "Tab" && home) {
      e.preventDefault();
      pickMode(cycleMode(mode, enabled, e.shiftKey ? -1 : 1));
      return;
    }
    if (
      e.key === "ArrowDown" &&
      props.onArrowDown &&
      e.currentTarget.selectionStart === text.length &&
      props.onArrowDown()
    ) {
      e.preventDefault();
      return;
    }
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      void submit();
    }
  };

  const placeholder = props.compacting
    ? t("composer.compacting")
    : props.running
    ? t("composer.running")
    : props.readOnly
      ? t("composer.readOnly")
      : !home
        ? t("composer.placeholder.session")
        : t(`composer.placeholder.${mode}`);
  const lines = Math.min(10, Math.max(1, text.split("\n").length));

  return (
    <div className="composer-wrap">
      {home && !searching && busy !== true && busy && (
        <div className="msg user sending">
          <div className="bubble">{busy}</div>
          <div className="queued-mark muted">Sending…</div>
        </div>
      )}
      {fileQuery != null && hits?.length === 0 && (
        <div className="mention-popup mention-none muted" role="status">
          {t("composer.noFile")}
        </div>
      )}
      {popupOpen && (
        <ul className="mention-popup" role="listbox" aria-label={fileQuery != null ? t("composer.files") : searching ? "search engines" : "agents"}>
          {fileQuery != null && (
            <li role="presentation" className="option-group">
              {t("composer.files")}
            </li>
          )}
          {suggestions.map((a, i) => (
            <li
              key={a.id}
              role="option"
              aria-selected={i === pick}
              className={`${i === pick ? "active" : ""}${a.agent && agentAlert(a.agent) ? " bad" : ""}`}
              onMouseDown={(e) => {
                e.preventDefault();
                choose(a.id);
              }}
            >
              {searching && <EngineIcon origin={a.origin ?? null} />}
              {a.agent && <AgentMark agent={a.agent} size={18} />}
              {a.file && <FileText size={14} />}
              <b>@{a.id}</b> {searching && <span>{a.title}</span>} <span className="option-hint">{a.hint}</span>
            </li>
          ))}
        </ul>
      )}
      <div
        className={`composer ${mode === "search" && home ? "is-search" : ""}${props.choosing ? " choosing" : ""}`}
        onDragOver={(e) => canImage && e.preventDefault()}
        onDrop={(e) => takeImages(e, e.dataTransfer.files)}
      >
        <textarea
          ref={ta}
          rows={lines}
          value={text}
          placeholder={placeholder}
          aria-label={placeholder}
          disabled={!!props.readOnly}
          hidden={props.choosing}
          onChange={(e) => {
            setText(e.target.value);
            setCaret(e.target.selectionStart);
            setPick(0);
            setPopupClosed(false);
          }}
          onSelect={(e) => setCaret(e.currentTarget.selectionStart)}
          onKeyDown={onKeyDown}
          onPaste={(e) => takeImages(e, e.clipboardData.files)}
        />
        {files.length + pictures.length + attached.length > 0 || several || tags.length > 0 || mentions.over ? (
          <ul className="chips" aria-label={t("composer.attachments")}>
            {mentions.over && (
              <li className="chip" role="alert">
                {t("composer.tooManyMentions")}
              </li>
            )}
            {tags.map((id) => (
              <li key={`tag-${id}`} className="chip tag" title={t("composer.tagTitle")}>
                @{id}
              </li>
            ))}
            {several &&
              (called.length > 4 ? (
                <li className="chip" role="alert">
                  {t("composer.tooMany")}
                </li>
              ) : (
                called.map((id, n) => (
                  <li key={`call-${id}`} className="chip" title={t("composer.calls")}>
                    {n + 1}. @{id}
                  </li>
                ))
              ))}
            {pictures.map((x) => (
              <li key={x.key} className="chip image-chip">
                {x.media_id ? (
                  <Suspense fallback={null}>
                    <LinkedImg url={`/api/media/${x.media_id}/content`} alt="" />
                  </Suspense>
                ) : (
                  <Loader2 size={14} className="spin" />
                )}
                {x.name}
                <button type="button" className="link" aria-label={`${t("composer.remove")} ${x.name}`} onClick={() => setPictures((p) => p.filter((y) => y !== x))}>
                  <X size={14} />
                </button>
              </li>
            ))}
            {attached.map((p) => (
              <li key={`@${p}`} className="chip" title={t("composer.mentionHint")}>
                <AtSign size={14} /> {p}
                <button type="button" className="link" aria-label={`${t("composer.remove")} ${p}`} onClick={() => setDropped((d) => [...d, p])}>
                  <X size={14} />
                </button>
              </li>
            ))}
            {files.map((f) => (
              <Fragment key={f.attachment_id}>
              <li className={f.saved || f.kept ? "chip saved" : "chip"} title={f.saved ? t("composer.savedHint") : f.kept ? t("composer.keptHint") : `${(f.chars ?? 0).toLocaleString()} ${t("composer.chars")}`}>
                <FileText size={14} /> <span className="chip-name">{f.name}</span>
                {f.saved && <span className="muted chip-note"> ({bytes(f.bytes ?? 0)}, {t("composer.saved")})</span>}
                {f.kept && <span className="muted chip-note"> ({bytes(f.bytes ?? 0)}, {t("composer.kept")})</span>}
                {f.truncated && <span className="muted"> ({t("composer.truncated")})</span>}
                <button
                  type="button"
                  className="link"
                  aria-label={`${t("composer.remove")} ${f.name}`}
                  onClick={() => setFiles((prev) => prev.filter((x) => x !== f))}
                >
                  <X size={14} />
                </button>
              </li>
              {f.note && <li className="chip-sentence muted">{f.note}</li>}
              </Fragment>
            ))}
          </ul>
        ) : null}
        <div className="composer-row">
          {mode !== "search" && (
            <>
              <button
                type="button"
                className="icon attach"
                aria-label={t("composer.attach")}
                title={canImage ? "Attach documents or images (images are kept in Media)" : t("composer.attachHint")}
                disabled={files.length + pictures.length >= MAX_ATTACHMENTS || uploading > 0}
                onClick={() => fileInput.current?.click()}
              >
                {uploading > 0 ? <Loader2 size={18} className="spin" /> : <Paperclip size={18} />}
              </button>
              <input
                ref={fileInput}
                type="file"
                multiple
                hidden
                onChange={(e) => {
                  // Images go with the message (lib/downscale); documents attach.
                  const all = [...(e.target.files ?? [])];
                  const images = canImage ? all.filter(isImage) : [];
                  addImages(images);
                  void addFiles(all.filter((f) => !images.includes(f)));
                  e.target.value = "";
                }}
              />
            </>
          )}
          {props.running && props.onStop && !props.readOnly ? (
            // Inside the box's row: a line under the strip would move the box when a turn starts or ends.
            <span className="composer-hint muted" title={t("composer.runningHint")}>
              {t("composer.runningHint")}
            </span>
          ) : (
            <span className="spacer" />
          )}
          {props.voice && mode !== "search" && (
            <button
              type="button"
              className={`icon mic ${rec}`}
              aria-label={rec === "recording" ? "stop dictation" : "dictate"}
              title={rec === "recording" ? "Stop and transcribe" : "Dictate (audio goes to this server's transcription service)"}
              disabled={rec === "transcribing"}
              onClick={() => void toggleMic()}
            >
              {rec === "recording" ? (
                <Square size={16} />
              ) : rec === "transcribing" ? (
                <Loader2 size={18} className="spin" />
              ) : (
                <Mic size={18} />
              )}
            </button>
          )}
          {(props.running || props.compacting) && props.onStop && (
            <button type="button" className="send stop" onClick={props.onStop} aria-label={t("composer.stop")}>
              <Square size={16} fill="currentColor" />
            </button>
          )}
          {/* While a turn runs, Send joins Stop once there is something to send. */}
          {(!((props.running || props.compacting) && props.onStop) || !!text.trim()) && (
            <button
              type="button"
              className="send"
              onClick={() => void submit()}
              disabled={!!busy || uploading > 0 || sendingImages || !text.trim() || !!props.readOnly}
              aria-label={t("composer.send")}
            >
              <ArrowUp size={18} />
            </button>
          )}
        </div>
      </div>
      <div className="composer-strip" role="group" aria-label={t("composer.strip")}>
        {/* A sentence, so a line of its own above the controls (they stay one row). */}
        {/* Why the commands control is disabled, said when it is clicked: a note on every running turn would move the box. */}
        {!home && props.exec?.busy && execNote && (
          <span className="approval-pending-note small muted" role="note" data-testid="exec-busy-note">
            {props.exec.busy}
          </span>
        )}
        {!home && props.approval && !props.readOnly && props.choosing && props.modeNote && (
          <span className="approval-pending-note small muted" role="note">
            {props.modeNote}
          </span>
        )}
        <div className="strip-controls">
          {mode !== "search" && props.agents.length > 0 && (
            <span
              className={`agent-pick${pickedAlert ? " bad" : ""}`}
              title={
                override ? `@${override} ${t("composer.forMessage")}` : stillWorking ? "Stop the agent to pick another" : (pickedAlert ?? undefined)
              }
            >
              <Select
                compact
                label="agent"
                disabled={stillWorking}
                value={props.agentId}
                options={pickable(props.agents, props.agentId).map((a) => ({
                  value: a.id,
                  label: a.name,
                  hint: marked(a, agentAlert(a) ? "model unavailable" : undefined),
                  icon: <AgentMark agent={a} size={18} state={a.id === props.agentId ? props.agentState : undefined} />,
                }))}
                onChange={(id) => {
                  props.onAgentChange(id);
                  ta.current?.focus();
                }}
              />
            </span>
          )}
          {/* What answers in this chat (it was in the chat's header): read-only. */}
          {!home && props.sessionModel && (
            <span className="strip-model" title={t("strip.model", { id: props.sessionModel })}>
              {rawModel(props.sessionModel)}
            </span>
          )}
          {!home && props.approval && !props.readOnly && (
            <Suspense fallback={null}>
              <ApprovalModePick
                value={props.approval.value}
                max={props.approval.max}
                host={props.approval.host}
                onChange={(m) => {
                  setErr(null);
                  props.approval!.onChange(m).catch((x) => setErr(x instanceof Error ? x.message : String(x)));
                  if (!props.choosing) ta.current?.focus();
                }}
              />
            </Suspense>
          )}
          {!home && props.exec && !props.readOnly && (
            <Suspense fallback={null}>
              <ExecSwitch
                value={props.exec.value}
                offered={props.exec.offered}
                mayHost={props.exec.mayHost}
                node={props.exec.node}
                approval={props.approval?.value ?? ""}
                busy={props.exec.busy}
                onBlocked={() => setExecNote(true)}
                onChange={(m) => {
                  setErr(null);
                  props.exec!.onChange(m).catch((x) => setErr(x instanceof Error ? x.message : String(x)));
                }}
              />
            </Suspense>
          )}
          {!home && props.steps && !props.readOnly && (
            <Suspense fallback={null}>
              <StepLimitPick
                value={props.steps.value}
                max={props.steps.max}
                onChange={(n) => {
                  setErr(null);
                  return props.steps!.onChange(n).catch((x) => setErr(x instanceof Error ? x.message : String(x)));
                }}
              />
            </Suspense>
          )}
          {override && override !== props.agentId && (
            <span className="pill override">
              <CornerDownRight size={14} /> @{override} {t("composer.forMessage")}
            </span>
          )}
          {home && mode !== "search" && offeredModels(props.models, model).length > 1 && (
            <select className="pill" aria-label="model" value={model} onChange={(e) => setModel(e.target.value)}>
              {offeredModels(props.models, model).map((m) => (
                <option key={m.id} value={m.id} disabled={m.unavailable}>
                  {modelLabel(m, props.models)}
                </option>
              ))}
            </select>
          )}
          {home && mode === "ask" && offered.some((m) => m.id !== model) && (
            <select
              className="pill"
              aria-label="compare with"
              title="Also answer with another model, side by side"
              value={compareWith ?? ""}
              onChange={(e) => setCompareWith(e.target.value || null)}
            >
              <option value="">compare: off</option>
              {offered
                .filter((m) => m.id !== model)
                .map((m) => (
                  <option key={m.id} value={m.id}>
                    vs {modelLabel(m, props.models)}
                  </option>
                ))}
            </select>
          )}
          {mode !== "search" && effortOk && (
            <select
              className="pill"
              aria-label="effort"
              value={effort ?? ""}
              onChange={(e) => setEffort((e.target.value || null) as Effort | null)}
            >
              <option value="">{t("effort.default")}</option>
              {EFFORTS.map((x) => (
                <option key={x} value={x}>
                  {x}
                </option>
              ))}
            </select>
          )}
          {searching && (
            <>
              <div className="seg" role="radiogroup" aria-label="search kind" onKeyDown={radioKeys}>
                <button
                  type="button"
                  role="radio"
                  aria-checked={kind === "text"}
                  tabIndex={radioTab(kind === "text")}
                  className={kind === "text" ? "on" : ""}
                  onClick={() => setSearchKind("text")}
                >
                  <Globe size={14} /> Web
                </button>
                <button
                  type="button"
                  role="radio"
                  aria-checked={kind === "image"}
                  tabIndex={radioTab(kind === "image")}
                  className={kind === "image" ? "on" : ""}
                  onClick={() => setSearchKind("image")}
                >
                  <Image size={14} /> Images
                </button>
              </div>
              <Select
                compact
                label="Search engine"
                value={engine?.id ?? ""}
                options={engines
                  .filter((e) => supports(e, kind))
                  .map((e) => ({
                    value: e.id,
                    label: e.name,
                    hint: abilities(e),
                    icon: <EngineIcon origin={engineOrigin(e)} />,
                  }))}
                onChange={(id) => {
                  setEnginePick((p) => ({ ...p, [kind]: id }));
                  ta.current?.focus();
                }}
              />
            </>
          )}
          {home && mode === "ask" && !inProject && (
            <button
              type="button"
              className={`pill incognito ${incognito ? "on" : ""}`}
              aria-pressed={incognito}
              title={t("composer.incognitoHint")}
              onClick={() => setIncognito((v) => !v)}
            >
              {t("composer.incognito")}
            </button>
          )}
          {home && (
            <div
              ref={track}
              className={`mode-switch ${auto ? "auto" : ""} ${thumbReady ? "ready" : ""}`}
              role="radiogroup"
              aria-label="mode"
              title={t("mode.hint")}
              onKeyDown={radioKeys}
            >
              <span className="mode-thumb" aria-hidden="true" />
              {MODES.filter((m) => !inProject || m !== "search").map((m) => {
                const disabled = !enabled.includes(m);
                return (
                  <button
                    key={m}
                    type="button"
                    role="radio"
                    aria-checked={mode === m}
                    tabIndex={radioTab(mode === m)}
                    className={mode === m ? "on" : ""}
                    disabled={disabled}
                    title={disabled ? (props.agenticBlocked ?? undefined) : undefined}
                    onClick={() => {
                      pickMode(m);
                      ta.current?.focus();
                    }}
                  >
                    {t(`mode.${m}`)}
                  </button>
                );
              })}
            </div>
          )}
        </div>
        {props.figures && <div className="strip-figures">{props.figures}</div>}
        {/* Room for the second line while another mode is on: the strip then
            grows downward only, and the box does not move (the home screen
            centres its column). The page's CSS gives it its height. */}
        {home && mode !== "agentic" && enabled.includes("agentic") && <div className="agentic-slot" aria-hidden="true" />}
        {home && mode === "agentic" && (
          <div className="composer-row agentic-row">
            <label>
              {t("composer.node")}
              <select
                className="pill"
                value={nodeId}
                onChange={(e) => {
                  setNodeId(e.target.value);
                  setHostFor(null);
                  unpick();
                }}
              >
                {props.workers.map((n) => (
                  <option key={n.node_id} value={n.node_id}>
                    {n.embedded ? `local (${shownName(n.hostname)})` : shownName(n.hostname)}
                  </option>
                ))}
              </select>
            </label>
            <Suspense fallback={null}>
              <ProjectPicker
                nodeId={nodeId}
                workers={props.workers}
                workspace={props.workspace}
                cwd={cwd}
                project={project}
                fixed={inProject}
                // `hostFor` names its node: another node's chat starts at the sandbox.
                // A folder names no node: the one chosen stays.
                onPick={(c) => {
                  if (c.nodeId) setNodeId(c.nodeId);
                  setPlace(c);
                }}
              />
            </Suspense>
            {worker && (
              <Suspense fallback={null}>
              <ExecChoice
                pickable={hostPickable}
                hostOff={props.hostRight?.level === "host" && !worker.exec_modes?.includes("host")}
                // A choice is for one node: another starts at the sandbox.
                key={worker.node_id}
                node={worker.embedded ? "local" : shownName(worker.hostname)}
                offered={worker.exec_modes ?? []}
                value={hostFor === worker.node_id ? "host" : undefined}
                onChange={(m) => setHostFor(m === "host" ? worker.node_id : null)}
              />
              </Suspense>
            )}
            {worker?.max_approval && (
              <Suspense fallback={null}>
                <ApprovalModePick
                  value={approvalPick ?? worker.default_approval ?? "auto"}
                  max={worker.max_approval}
                  host={hostPickable && hostFor === nodeId}
                  onChange={(m) => setApprovalFor({ node: nodeId, mode: m })}
                />
              </Suspense>
            )}
          </div>
        )}
      </div>
      {home && props.agenticBlocked && <div className="hint muted">{props.agenticBlocked}</div>}
      {props.readOnly && <div className="hint muted">{t("conv.readOnly", { reason: props.readOnly })}</div>}
      {err && <div className="error-text">{err}</div>}
    </div>
  );
}

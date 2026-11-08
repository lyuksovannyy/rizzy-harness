import { ArrowUp, CornerDownRight, FileText, Globe, Image, Loader2, Mic, Paperclip, Square, X } from "lucide-react";
import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { t } from "../lib/i18n";
import { pickable } from "../lib/agents";
import { activeMention, filterAgents, insertMention, leadingMention } from "../lib/mentions";
import { cycleMode, enabledModes, MODES, type Mode } from "../lib/modes";
import { modelLabel, offeredModels, resolveModel } from "../lib/models";
import { radioKeys, radioTab } from "../lib/radios";
import { api } from "../api";
import { abilities, allEngines, type CustomEngine, engineByUrl, engineMention, engineOrigin, type SearchKind, supports } from "../lib/engines";
import { EngineIcon } from "./EngineIcon";
import { Select } from "./Select";
import { AgentMark, agentAlert } from "./AgentAvatar";
import type { Agent, AvatarState, Effort, ModelInfo, NodeInfo, UploadedAttachment } from "../types";

const FOLDERS_KEY = "harness.folders";
const EFFORTS: Effort[] = ["low", "medium", "high", "max"];

function recentFolders(): string[] {
  try {
    return JSON.parse(localStorage.getItem(FOLDERS_KEY) ?? "[]") as string[];
  } catch {
    return [];
  }
}

function rememberFolder(f: string) {
  const list = [f, ...recentFolders().filter((x) => x !== f)].slice(0, 8);
  try {
    localStorage.setItem(FOLDERS_KEY, JSON.stringify(list));
  } catch {
    /* storage unavailable */
  }
}

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
}

const MAX_ATTACHMENTS = 5;

export function Composer(props: {
  /** `home` shows the mode switch and model picker; `session` is a reply box. */
  where: "home" | "session";
  agents: Agent[];
  models: ModelInfo[];
  workers: NodeInfo[];
  /** Selected agent (home: the choice for the new chat; session: the session's agent). */
  agentId: string;
  onAgentChange: (id: string) => void;
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
  draftKey?: string;
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
}) {
  const home = props.where === "home";
  const [text, setText] = useState(() =>
    props.draftKey ? (localStorage.getItem(props.draftKey) ?? "") : "",
  );
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
  const [nodeId, setNodeId] = useState(props.workers[0]?.node_id ?? "");
  const [cwd, setCwd] = useState(() => recentFolders()[0] ?? "");
  const [caret, setCaret] = useState(0);
  const [pick, setPick] = useState(0);
  const [popupClosed, setPopupClosed] = useState(false);
  const [busy, setBusy] = useState(false);
  const [incognito, setIncognito] = useState(false);
  const [compareWith, setCompareWith] = useState<string | null>(null);
  const [searchKind, setSearchKind] = useState<SearchKind>("text");
  const [enginePick, setEnginePick] = useState<Partial<Record<SearchKind, string>>>({});
  const [files, setFiles] = useState<UploadedAttachment[]>([]);
  const [uploading, setUploading] = useState(0);
  const fileInput = useRef<HTMLInputElement>(null);
  const [rec, setRec] = useState<"idle" | "recording" | "transcribing">("idle");
  const recorder = useRef<MediaRecorder | null>(null);

  const toggleMic = async () => {
    if (rec === "recording") {
      recorder.current?.stop();
      return;
    }
    setErr(null);
    try {
      const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
      const r = new MediaRecorder(stream);
      const chunks: Blob[] = [];
      r.ondataavailable = (e) => chunks.push(e.data);
      r.onstop = async () => {
        stream.getTracks().forEach((t) => t.stop());
        setRec("transcribing");
        try {
          const said = await api.transcribe(new Blob(chunks, { type: r.mimeType || "audio/webm" }));
          if (said) setText((t) => (t.trim() ? `${t.trimEnd()} ${said}` : said));
          ta.current?.focus();
        } catch (x) {
          setErr(x instanceof Error ? x.message : String(x));
        } finally {
          setRec("idle");
        }
      };
      recorder.current = r;
      r.start();
      setRec("recording");
    } catch (x) {
      setErr(`microphone: ${x instanceof Error ? x.message : String(x)}`);
    }
  };
  const [err, setErr] = useState<string | null>(null);
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
    if (!props.workers.some((w) => w.node_id === nodeId)) setNodeId(props.workers[0]?.node_id ?? "");
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
  useEffect(() => {
    if (!props.draftKey) return;
    try {
      if (text) localStorage.setItem(props.draftKey, text);
      else localStorage.removeItem(props.draftKey);
    } catch {
      /* storage unavailable */
    }
  }, [text, props.draftKey]);
  useEffect(() => {
    ta.current?.focus();
  }, []);

  const enabled = useMemo(() => enabledModes(props.agenticBlocked), [props.agenticBlocked]);
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

  const mention = popupClosed ? null : activeMention(text, caret);
  const suggestions: { id: string; title: string; hint: string; origin?: string | null; agent?: Agent }[] = !mention
    ? []
    : searching
      ? engines
          .filter((e) => e.id.startsWith(mention.query.toLowerCase()) || e.name.toLowerCase().startsWith(mention.query.toLowerCase()))
          .slice(0, 8)
          .map((e) => ({ id: e.id, title: e.name, hint: abilities(e), origin: engineOrigin(e) }))
      : filterAgents(pickable(props.agents), mention.query).map((a) => ({ id: a.id, title: a.id, hint: a.description || a.name, agent: a }));
  const popupOpen = suggestions.length > 0;
  const pickedAgent = props.agents.find((a) => a.id === props.agentId);
  const pickedAlert = pickedAgent ? agentAlert(pickedAgent) : null;
  const effortModel = home ? model : props.sessionModel;
  const effortOk = resolveModel(props.models, effortModel)?.effort ?? false;

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
    if (!body || busy || uploading || props.running || props.readOnly) return;
    if (mode === "agentic" && (!cwd.trim() || !nodeId)) {
      setErr(t("composer.folder"));
      return;
    }
    setBusy(true);
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
      });
      if (mode === "agentic") rememberFolder(cwd.trim());
      if (home) props.onLearn?.(body, mode);
      setText("");
      setFiles([]);
      setManual(false);
    } catch (x) {
      setErr(x instanceof Error ? x.message : String(x));
    } finally {
      setBusy(false);
    }
  };

  const addFiles = async (list: FileList | null) => {
    if (!list) return;
    const picked = Array.from(list).slice(0, MAX_ATTACHMENTS - files.length);
    setErr(null);
    for (const f of picked) {
      setUploading((n) => n + 1);
      try {
        const up = await api.upload(f);
        setFiles((prev) => [...prev, up]);
      } catch (x) {
        setErr(`${f.name}: ${x instanceof Error ? x.message : String(x)}`);
      } finally {
        setUploading((n) => n - 1);
      }
    }
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

  const placeholder = props.running
    ? t("composer.running")
    : props.readOnly
      ? t("composer.readOnly")
      : !home
        ? t("composer.placeholder.session")
        : t(`composer.placeholder.${mode}`);
  const lines = Math.min(10, Math.max(1, text.split("\n").length));

  return (
    <div className="composer-wrap">
      {popupOpen && (
        <ul className="mention-popup" role="listbox" aria-label={searching ? "search engines" : "agents"}>
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
              <b>@{a.id}</b> {searching && <span>{a.title}</span>} <span className="option-hint">{a.hint}</span>
            </li>
          ))}
        </ul>
      )}
      <div className={`composer ${mode === "search" && home ? "is-search" : ""}`}>
        <textarea
          ref={ta}
          rows={lines}
          value={text}
          placeholder={placeholder}
          aria-label={placeholder}
          disabled={!!props.readOnly}
          onChange={(e) => {
            setText(e.target.value);
            setCaret(e.target.selectionStart);
            setPick(0);
            setPopupClosed(false);
          }}
          onSelect={(e) => setCaret(e.currentTarget.selectionStart)}
          onKeyDown={onKeyDown}
        />
        {files.length > 0 && (
          <ul className="chips" aria-label={t("composer.attachments")}>
            {files.map((f) => (
              <li key={f.attachment_id} className="chip" title={`${f.chars.toLocaleString()} ${t("composer.chars")}`}>
                <FileText size={14} /> {f.name}
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
            ))}
          </ul>
        )}
        <div className="composer-row">
          {mode !== "search" && (
            <>
              <button
                type="button"
                className="icon attach"
                aria-label={t("composer.attach")}
                title={t("composer.attachHint")}
                disabled={files.length >= MAX_ATTACHMENTS || uploading > 0}
                onClick={() => fileInput.current?.click()}
              >
                {uploading > 0 ? <Loader2 size={18} className="spin" /> : <Paperclip size={18} />}
              </button>
              <input
                ref={fileInput}
                type="file"
                multiple
                hidden
                accept=".txt,.md,.markdown,.csv,.json,.xml,.yaml,.yml,.toml,.html,.log,.pdf,.rs,.ts,.tsx,.js,.py,.go,.java,.c,.h,.cpp,.sh,.sql,text/*,application/pdf"
                onChange={(e) => {
                  void addFiles(e.target.files);
                  e.target.value = "";
                }}
              />
            </>
          )}
          {mode !== "search" && props.agents.length > 0 && (
            <span
              className={`agent-pick${pickedAlert ? " bad" : ""}`}
              title={override ? `@${override} ${t("composer.forMessage")}` : (pickedAlert ?? undefined)}
            >
              <Select
                compact
                label="agent"
                value={props.agentId}
                options={pickable(props.agents, props.agentId).map((a) => ({
                  value: a.id,
                  label: a.name,
                  hint: agentAlert(a) ? "model unavailable" : undefined,
                  icon: <AgentMark agent={a} size={18} state={a.id === props.agentId ? props.agentState : undefined} />,
                }))}
                onChange={(id) => {
                  props.onAgentChange(id);
                  ta.current?.focus();
                }}
              />
            </span>
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
          <span className="spacer" />
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
          {home && mode === "ask" && (
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
              {MODES.map((m) => {
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
          {props.running && props.onStop ? (
            <button type="button" className="send stop" onClick={props.onStop} aria-label={t("composer.stop")}>
              <Square size={16} fill="currentColor" />
            </button>
          ) : (
            <button
              type="button"
              className="send"
              onClick={() => void submit()}
              disabled={busy || uploading > 0 || !text.trim() || !!props.readOnly}
              aria-label={t("composer.send")}
            >
              <ArrowUp size={18} />
            </button>
          )}
        </div>
        {home && mode === "agentic" && (
          <div className="composer-row agentic-row">
            <label>
              {t("composer.node")}
              <select className="pill" value={nodeId} onChange={(e) => setNodeId(e.target.value)}>
                {props.workers.map((n) => (
                  <option key={n.node_id} value={n.node_id}>
                    {n.embedded ? `local (${n.hostname})` : n.hostname}
                  </option>
                ))}
              </select>
            </label>
            <label className="grow">
              {t("composer.folder")}
              <input
                className="mono"
                list="recent-folders"
                placeholder="/home/me/project"
                value={cwd}
                onChange={(e) => setCwd(e.target.value)}
              />
              <datalist id="recent-folders">
                {recentFolders().map((f) => (
                  <option key={f} value={f} />
                ))}
              </datalist>
            </label>
          </div>
        )}
      </div>
      {home && props.agenticBlocked && <div className="hint muted">{props.agenticBlocked}</div>}
      {props.readOnly && <div className="hint muted">{t("conv.readOnly", { reason: props.readOnly })}</div>}
      {err && <div className="error-text">{err}</div>}
    </div>
  );
}

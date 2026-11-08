import { DEFAULT_IMAGE_SEARCH } from "../lib/engines";
import { lazy, Suspense, useCallback, useEffect, useRef, useState } from "react";
import { greeting } from "../lib/greeting";
import { t } from "../lib/i18n";
import type { Agent, ModelInfo, NodeInfo, Notice, Prefs, AttentionItem, NodeAttention, RoutineAttention } from "../types";
import type { Mode } from "../lib/modes";
import { Composer, type Submission } from "./Composer";
import { Attention } from "./Attention";
import { Notices } from "./Notices";
import { Similar, type SimilarHandle } from "./Similar";
import type { History } from "../bg/topics";
import { readBackdropMirror } from "../lib/appearance";
import { useFeatures } from "../lib/features";
import { MEMORY_HASH } from "../lib/routes";

// The mode backgrounds load on their own, and not at all when turned off.
const Backdrop = lazy(() => import("../bg/Backdrop"));

/** How long a mode the bar picked must hold before the background follows. */
const AUTO_SETTLE_MS = 700;

export function Home(props: {
  username: string;
  timeZone?: string | null;
  agents: Agent[];
  defaultAgent: string;
  models: ModelInfo[];
  workers: NodeInfo[];
  prefs: Prefs | null;
  agenticBlocked: string | null;
  /** The active workspace, which the similar chats come from. */
  workspace: string | null;
  onSubmit: (s: Submission) => Promise<void>;
  predict: (text: string) => Mode | null;
  onLearn: (text: string, mode: Mode) => void;
  onOpen: (sessionId: string) => void;
  /** Memory proposals waiting for the user. */
  toReview?: number;
  voice?: boolean;
  attention?: AttentionItem[];
  nodeAttention?: NodeAttention[];
  /** Routine results not seen yet, shown below the chats waiting for the user. */
  routines?: RoutineAttention[];
  /** Lasting problems (a model gone from its provider): always shown, above the similar chats. */
  notices?: Notice[];
  /** New per home visit: the background re-rolls with it. */
  seed: number;
  history: History;
}) {
  const [similarCount, setSimilarCount] = useState(0);
  const [typed, setTyped] = useState<{ text: string; mode: Mode }>({ text: "", mode: "ask" });
  const onTextChange = useCallback((text: string, mode: Mode) => setTyped({ text, mode }), []);
  // The background follows a mode the user picked at once; one the
  // classifier picked only once it has held, so it cannot flicker while
  // they type.
  const [bgMode, setBgMode] = useState<Mode>("ask");
  const settle = useRef<ReturnType<typeof setTimeout> | null>(null);
  const onModeChange = useCallback((mode: Mode, by: "user" | "auto") => {
    if (settle.current) clearTimeout(settle.current);
    settle.current = null;
    if (by === "user") setBgMode(mode);
    else settle.current = setTimeout(() => setBgMode(mode), AUTO_SETTLE_MS);
  }, []);
  useEffect(() => () => {
    if (settle.current) clearTimeout(settle.current);
  }, []);
  // Until the preferences arrive, the choice the last load saved.
  const [mirror] = useState(readBackdropMirror);
  // Images in chats: whether this server takes them here.
  const vision = !!useFeatures(props.workspace)?.media.vision_images;
  const backdrop = props.prefs ? (props.prefs.backdrop ?? "animated") : mirror;
  const similar = useRef<SimilarHandle>(null);
  const [agentId, setAgentId] = useState(props.defaultAgent);
  useEffect(() => setAgentId(props.defaultAgent), [props.defaultAgent]);
  // Re-render the greeting if the page stays open across a time bucket.
  const [now, setNow] = useState(() => new Date());
  useEffect(() => {
    const id = setInterval(() => setNow(new Date()), 60_000);
    return () => clearInterval(id);
  }, []);

  return (
    <section className="home">
      {backdrop !== "off" && (
        <Suspense fallback={null}>
          <Backdrop
            mode={bgMode}
            seed={props.seed}
            pref={backdrop}
            typing={typed.text.trim() !== ""}
            history={props.history}
          />
        </Suspense>
      )}
      <div className="column">
        <h1 className="greeting">{greeting(now, props.username, props.timeZone)}</h1>
        <p className="muted subtitle">{t("home.subtitle")}</p>
        <Composer
          where="home"
          agents={props.agents}
          models={props.models}
          workers={props.workers}
          agentId={agentId}
          onAgentChange={setAgentId}
          defaultModel={props.prefs?.default_model}
          agenticBlocked={props.agenticBlocked}
          onSubmit={props.onSubmit}
          predict={props.predict}
          onLearn={props.onLearn}
          onTextChange={onTextChange}
          onModeChange={onModeChange}
          onArrowDown={() => similar.current?.focusFirst() ?? false}
          voice={props.voice}
          images={vision ? { workspace: props.workspace, pending: true } : null}
          search={
            props.prefs
              ? {
                  text: props.prefs.search_url,
                  image: props.prefs.image_search_url ?? DEFAULT_IMAGE_SEARCH,
                  custom: props.prefs.custom_engines,
                }
              : undefined
          }
        />
        <Notices notices={props.notices ?? []} agents={props.agents} />
        <Similar
          ref={similar}
          text={typed.text}
          mode={typed.mode}
          workspace={props.workspace}
          onOpen={props.onOpen}
          onCount={setSimilarCount}
        />
        {similarCount === 0 && (
          <Attention
            items={props.attention ?? []}
            nodes={props.nodeAttention}
            routines={props.routines}
            onOpen={props.onOpen}
            timeZone={props.timeZone}
          />
        )}
        {!!props.toReview && (
          <a className="review-link small" href={MEMORY_HASH}>
            {props.toReview === 1 ? t("home.toReviewOne") : t("home.toReview", { n: String(props.toReview) })}
          </a>
        )}
      </div>
    </section>
  );
}

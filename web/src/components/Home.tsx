import { useCallback, useEffect, useRef, useState } from "react";
import { greeting } from "../lib/greeting";
import { t } from "../lib/i18n";
import type { Agent, ModelInfo, NodeInfo, Prefs } from "../types";
import type { Mode } from "../lib/modes";
import { Composer, type Submission } from "./Composer";
import { Similar, type SimilarHandle } from "./Similar";

export function Home(props: {
  username: string;
  timeZone?: string | null;
  agents: Agent[];
  defaultAgent: string;
  models: ModelInfo[];
  workers: NodeInfo[];
  prefs: Prefs | null;
  agenticBlocked: string | null;
  onSubmit: (s: Submission) => Promise<void>;
  predict: (text: string) => Mode | null;
  onLearn: (text: string, mode: Mode) => void;
  onOpen: (sessionId: string) => void;
}) {
  const [typed, setTyped] = useState<{ text: string; mode: Mode }>({ text: "", mode: "ask" });
  const onTextChange = useCallback((text: string, mode: Mode) => setTyped({ text, mode }), []);
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
          onArrowDown={() => similar.current?.focusFirst() ?? false}
        />
        <Similar ref={similar} text={typed.text} mode={typed.mode} onOpen={props.onOpen} />
      </div>
    </section>
  );
}

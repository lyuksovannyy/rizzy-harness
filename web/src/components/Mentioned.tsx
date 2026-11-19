// A sent message's text with its resolved mentions drawn as chips (X.37):
// an agent that was called has its mark and a solid chip, a plain mention
// (it called nobody) is quieter, a sub-agent-only agent (a tag: the answering
// agent may run it, nobody is called) has a dashed outline, and the person's own name is a plain chip
// (nobody is asked about it). The text is otherwise the text. The rule is
// lib/mentionScan.ts, the mirror of the server's.
import { type ReactNode, useEffect, useState } from "react";
import { loginName } from "../lib/calledNow";
import { t } from "../lib/i18n";
import "../lib/i18nPages";
import { resolver, scanMentions } from "../lib/mentionScan";
import type { Agent } from "../types";
import { AgentMark } from "./AgentAvatar";

export function Mentioned(props: {
  text: string;
  agents: Agent[];
  /** The agents the server recorded for this message (`user_message.triggers`); none: it answered as one message to one agent. */
  triggers?: string[];
}) {
  const [login, setLogin] = useState<string | undefined>();
  const has = props.text.includes("@");
  useEffect(() => {
    if (has) loginName().then(setLogin);
  }, [has]);
  if (!has) return <>{props.text}</>;
  const agents = props.agents;
  const m = scanMentions(props.text, resolver(agents, login));
  if (!m.found.length) return <>{props.text}</>;
  // Several agents were asked only when the server recorded them; otherwise
  // the first agent called answered (an API token's or a routine's message).
  const asked = props.triggers?.length ? props.triggers : m.called.slice(0, 1);
  const out: ReactNode[] = [];
  let at = 0;
  for (const f of m.found) {
    out.push(props.text.slice(at, f.start));
    const tag = f.who.startsWith("tag:");
    const id = f.who.startsWith("agent:") ? f.who.slice(6) : tag ? f.who.slice(4) : null;
    const agent = id ? agents.find((a) => a.id === id) : undefined;
    const calls = !tag && !!id && f.trigger && asked.includes(id);
    const name = agent?.name ?? id ?? "";
    out.push(
      <span
        key={f.start}
        className={`mention ${calls ? "called" : tag ? "tag" : "plain"}`}
        title={calls ? t("conv.mentionCalls", { name }) : tag ? t("conv.mentionTag", { name }) : id ? t("conv.mentionPlain", { name }) : t("conv.mentionYou")}
      >
        {agent && calls && <AgentMark agent={agent} size={14} />}
        {props.text.slice(f.start, f.end)}
      </span>,
    );
    at = f.end;
  }
  out.push(props.text.slice(at));
  return <>{out}</>;
}

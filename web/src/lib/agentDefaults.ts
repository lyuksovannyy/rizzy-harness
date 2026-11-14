// The agent a new chat gets when the person has not picked one: the
// default of the chat's mode. Agentic chats have their own default (the
// Software Developer unless a setting says otherwise), because the chat
// default (the Assistant) has no shell or file tools. The picker shows the
// agent that will run, and that agent is what is sent.
import { useState } from "react";
import type { Mode } from "./modes";
import type { Prefs } from "../types";

export interface AgentDefaults {
  chat: string;
  /** null: the setting names an agent that cannot run Agentic chats. */
  agentic: string | null;
}

/** The person's own defaults, else the workspace's or the server's (the
 * server already folds the workspace's into `server`). */
export function agentDefaults(prefs: Prefs | null, server: AgentDefaults): AgentDefaults {
  return {
    chat: prefs?.default_agent ?? server.chat,
    agentic: prefs?.default_agentic_agent ?? server.agentic,
  };
}

/** The default for `mode`; without a usable Agentic one, the chat's. */
export const defaultFor = (d: AgentDefaults, mode: Mode | "chat"): string => (mode === "agentic" && d.agentic) || d.chat;

/** A new chat's agent: the person's pick, else the default of the mode
 * the composer reports (`onMode`). */
export function useNewChatAgent(d: AgentDefaults) {
  const [picked, pick] = useState<string | null>(null);
  const [mode, onMode] = useState<Mode>("ask");
  return { agentId: picked ?? defaultFor(d, mode), pick, onMode };
}

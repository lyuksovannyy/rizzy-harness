// What a rendered message knows about the conversation around it.
import { createContext } from "react";
import type { Provenance } from "./links";

export interface MessageInfo {
  /** URL keys from user text and tool output before this message. */
  provenance: Provenance;
  /** URL keys of verified sources (tool_call_finished sources); such links
   * are styled as sources and need no confirm. */
  sources: Provenance;
}

export const NONE: Provenance = { has: () => false };

export const MessageCtx = createContext<MessageInfo>({ provenance: NONE, sources: NONE });

/** False while the message is still streaming: highlighting and math wait. */
export const DoneCtx = createContext(true);

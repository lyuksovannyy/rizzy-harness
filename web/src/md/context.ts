// What a rendered message knows about the conversation around it.
import { createContext } from "react";
import type { Source, UiBlock } from "../types";
import type { Provenance } from "./links";

export interface MessageInfo {
  /** Link keys (see linkKey) from user text and web tool results before
   * this message. */
  provenance: Provenance;
  /** Link keys of verified sources (tool_call_finished sources); such links
   * are styled as sources and need no confirm. */
  sources: Provenance;
  /** Citation label (`S4`) to the source a web tool gave it before this
   * message. */
  cites: ReadonlyMap<string, Source>;
}

export const NONE: Provenance = { has: () => false };

export const NO_CITES: ReadonlyMap<string, Source> = new Map();

export const MessageCtx = createContext<MessageInfo>({ provenance: NONE, sources: NONE, cites: NO_CITES });

/** False while the message is still streaming: highlighting and math wait. */
export const DoneCtx = createContext(true);

/** The message's validated `ui:` blocks (assistant_message.blocks) and who
 * wrote it, for the `ui-block` elements remark-ui makes. */
export interface UiInfo {
  blocks: readonly UiBlock[];
  /** The agent's display name; null reads "the assistant". */
  from: string | null;
}

export const NO_BLOCKS: readonly UiBlock[] = [];

export const UiCtx = createContext<UiInfo>({ blocks: NO_BLOCKS, from: null });

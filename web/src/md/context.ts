// What a rendered message knows about the conversation around it.
import { createContext } from "react";
import type { Source, UiBlock } from "../types";
import type { ProgressSlots } from "../ui/blocks";
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
  /** The message the blocks belong to (for its progress slots). */
  messageId: string | null;
  blocks: readonly UiBlock[];
  /** The agent's display name; null reads "the assistant". */
  from: string | null;
  /** The message stopped mid-stream (its turn was cancelled, failed or
   * interrupted), so Rust never stored its blocks. */
  cut: boolean;
}

export const NO_BLOCKS: readonly UiBlock[] = [];

export const UiCtx = createContext<UiInfo>({ messageId: null, blocks: NO_BLOCKS, from: null, cut: false });

/** Where each progress block of the conversation stands in its turn
 * (ui/blocks.ts progressSlots). Conversation provides it above every
 * message, so a new slot re-renders only the blocks, not the Markdown. */
export const ProgressCtx = createContext<ProgressSlots>(new Map());

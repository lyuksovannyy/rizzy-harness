// The cards for `ui:` blocks, in their own lazy chunk: a chat without
// blocks never loads them. Each spec was validated and cleaned in Rust and
// every string renders as React text (no Markdown, no links). The chart
// is a further chunk, loaded only for a message with one; until it arrives
// the host's skeleton stands in.
import { lazy } from "react";
import type { UiBlock as Block } from "../types";
import type { ProgressSlot } from "./blocks";
import Choices from "./Choices";
import Form from "./Form";
import { PREVIEW, type InputProps } from "./Input";
import Progress from "./Progress";

const Chart = lazy(() => import("./Chart"));

/** `slot` places a progress block in its turn (full or snapshot, ended);
 * `input` is where a question or form stands and where its answer goes. */
export default function UiBlock({ block, slot, input }: { block: Block; slot?: ProgressSlot; input?: InputProps }) {
  const answer = input ?? { sessionId: null, messageId: null, blockId: block.id, info: PREVIEW };
  switch (block.type) {
    case "choices":
      return block.spec ? <Choices spec={block.spec} input={answer} /> : null;
    case "form":
      return block.spec ? <Form spec={block.spec} input={answer} /> : null;
    case "progress":
      return block.spec ? <Progress spec={block.spec} slot={slot} /> : null;
    case "chart":
      return block.spec ? <Chart spec={block.spec} /> : null;
  }
}

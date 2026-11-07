// A user message that answers a question or form (rich_plan §6.5): the
// answer as the user picked it (labels from the block, values as typed),
// then their note. Without the block (one that changed) it
// is the message text, which is the canonical answer and the note.
import { rt } from "../md/strings";
import type { FormReply, Item, UiBlock } from "../types";
import { answerLines, answerTitle } from "../ui/answer";

/** The block a reply answers, if the conversation still shows it. */
export function repliedBlock(items: readonly Item[], reply: FormReply): UiBlock | undefined {
  const asked = items.find((it) => it.type === "assistant" && it.message_id === reply.message_id);
  return asked?.type === "assistant" ? asked.blocks?.find((b) => b.id === reply.block_id && !!b.spec) : undefined;
}

export function ReplyBubble({ text, reply, block }: { text: string; reply: FormReply; block?: UiBlock }) {
  if (!block) return <div className="bubble">{text}</div>;
  // The canonical answer has no blank line (typed text is JSON-quoted), so
  // the note is whatever follows the first one.
  const gap = text.indexOf("\n\n");
  const note = gap < 0 ? "" : text.slice(gap + 2);
  return (
    <div className="bubble reply">
      <div className="reply-head">{rt("ui.answered", { title: answerTitle(block) })}</div>
      <ul className="reply-values">
        {answerLines(block, reply.values).map((l, i) => (
          <li key={i}>
            {l.label && <span className="reply-label">{l.label}: </span>}
            {l.value}
          </li>
        ))}
      </ul>
      {note && <p className="reply-note">{note}</p>}
    </div>
  );
}

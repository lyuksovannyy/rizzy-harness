// What a `remember` call did, in the harness's words: the model proposed a
// memory, which waits on the Memory page until the user keeps or discards
// it (rich_plan §13: no inline Keep/Discard). The fact is the model's text
// and is stored as written, so hidden characters show as codes here: the
// user should see what would go into future chats.
import { BookmarkPlus } from "lucide-react";
import { showHidden } from "../md/hidden";
import { rt } from "../md/strings";

export function MemoryNotice({ text }: { text: string }) {
  return (
    <div className="tool tool-memory" role="note">
      <BookmarkPlus size={14} aria-hidden="true" className="memory-icon" />
      <div className="memory-body">
        <div>
          {rt("memory.proposed")} <span className="memory-fact">“{showHidden(text.trim())}”</span>
        </div>
        <div className="muted small">
          {rt("memory.pending")}{" "}
          <a className="memory-review" href="#/settings/memory">
            {rt("memory.review")}
          </a>
        </div>
      </div>
    </div>
  );
}

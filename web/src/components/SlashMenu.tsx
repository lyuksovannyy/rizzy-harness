// The composer's `/` menu (X.65): the built-in commands and the skills the
// chat can ask for, above the box. Lazy: loaded on the first `/` of a message.
// Choosing only completes the text (`/name `); nothing is sent, run or read.
// The keys come from the composer's own key handler through `keys`.
import { useEffect, useState } from "react";
import { BUILTINS, complete, loadSkills, rank, type SlashEntry, shownDescription } from "../lib/slashMenu";

export interface SlashKeys {
  current: ((e: React.KeyboardEvent) => boolean) | null;
}

export default function SlashMenu(props: {
  chatId: string;
  query: string;
  text: string;
  running: boolean;
  keys: SlashKeys;
  onPick: (text: string, caret: number) => void;
  onClose: () => void;
}) {
  const { chatId, query, text, running, keys, onPick, onClose } = props;
  const [skills, setSkills] = useState<SlashEntry[]>([]);
  const [pick, setPick] = useState(0);
  useEffect(() => {
    const ctl = new AbortController();
    loadSkills(chatId, running, ctl.signal).then(
      (s) => setSkills(s),
      () => {},
    );
    return () => ctl.abort();
    // Once per opening of the menu: a turn that starts or ends meanwhile does not ask again.
  }, [chatId]);
  useEffect(() => setPick(0), [query]);
  const list = rank([...BUILTINS, ...skills], query);
  const at = Math.min(pick, list.length - 1);
  const choose = (e: SlashEntry) => {
    const r = complete(text, e.name);
    onPick(r.text, r.caret);
  };
  useEffect(() => {
    keys.current = (e) => {
      if (!list.length || e.ctrlKey || e.metaKey || e.altKey) return false;
      if (e.key === "ArrowDown" || e.key === "ArrowUp") {
        e.preventDefault();
        setPick((at + (e.key === "ArrowDown" ? 1 : -1) + list.length) % list.length);
        return true;
      }
      if ((e.key === "Enter" && !e.shiftKey) || e.key === "Tab") {
        const a = list[at]!;
        // `/name` typed in full and Enter: the message, as before.
        if (e.key === "Enter" && a.name === query) return false;
        e.preventDefault();
        choose(a);
        return true;
      }
      if (e.key === "Escape") {
        e.preventDefault();
        onClose();
        return true;
      }
      return false;
    };
    return () => {
      keys.current = null;
    };
  });
  if (!list.length) return null;
  const active = list[at]!;
  return (
    <>
      <ul className="mention-popup slash-menu" role="listbox" aria-label="Commands and skills">
        {list.map((e, i) => (
          <li
            key={e.name}
            id={`slash-opt-${i}`}
            role="option"
            aria-selected={i === at}
            className={i === at ? "active" : ""}
            onMouseDown={(ev) => {
              ev.preventDefault();
              choose(e);
            }}
          >
            <b>/{e.name}</b> {e.hint && <span className="option-hint">{e.hint}</span>}{" "}
            <span className="option-hint">{shownDescription(e.description)}</span>
          </li>
        ))}
      </ul>
      <div className="sr-only" role="status" aria-live="polite">
        {`/${active.name}${active.hint ? ` ${active.hint}` : ""}, ${shownDescription(active.description)}, ${at + 1} of ${list.length}`}
      </div>
    </>
  );
}

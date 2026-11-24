// The composer's `/` menu (X.65): the built-in commands and the skills the
// chat can ask for, above the box. Lazy: loaded on the first `/` of a message.
// Choosing only completes the text (`/name `); nothing is sent, run or read.
// The keys come from the composer's own key handler through `keys`.
// Nothing is highlighted when the menu opens: Enter keeps doing what it did
// (send the text, or run `/compact`) until the person moves the highlight
// with Up or Down; Tab completes the first entry, or the highlighted one.
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
  const [pick, setPick] = useState<string | null>(null);
  // The highlight follows the entry, not its place: the skills arrive after the first paint.
  useEffect(() => {
    const ctl = new AbortController();
    loadSkills(chatId, running, ctl.signal).then(
      (s) => setSkills(s),
      () => {},
    );
    return () => ctl.abort();
    // Once per opening of the menu: a turn that starts or ends meanwhile does not ask again.
  }, [chatId]);
  useEffect(() => setPick(null), [query]);
  const list = rank([...BUILTINS, ...skills], query);
  const found = pick == null ? -1 : list.findIndex((e) => e.name === pick);
  const at = found < 0 ? null : found;
  const choose = (e: SlashEntry) => {
    const r = complete(text, e.name);
    onPick(r.text, r.caret);
  };
  useEffect(() => {
    keys.current = (e) => {
      if (!list.length || e.ctrlKey || e.metaKey || e.altKey) return false;
      if (e.key === "ArrowDown" || e.key === "ArrowUp") {
        e.preventDefault();
        const d = e.key === "ArrowDown" ? 1 : -1;
        setPick(list[at == null ? (d > 0 ? 0 : list.length - 1) : (at + d + list.length) % list.length]!.name);
        return true;
      }
      // Tab completes; Enter only completes what the person moved to, else it is Enter as before.
      if (e.key === "Tab" || (e.key === "Enter" && !e.shiftKey && at != null)) {
        e.preventDefault();
        choose(list[at ?? 0]!);
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
  const active = at == null ? null : list[at]!;
  return (
    <>
      <ul className="mention-popup slash-menu" role="listbox" aria-label="Commands and skills">
        {list.map((e, i) => (
          <li
            key={e.name}
            id={`slash-opt-${i}`}
            role="option"
            aria-selected={at != null && i === at}
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
        {active
          ? `/${active.name}${active.hint ? ` ${active.hint}` : ""}, ${shownDescription(active.description)}, ${at! + 1} of ${list.length}`
          : `${list.length} ${list.length === 1 ? "entry" : "entries"}. Tab to complete, arrows to choose.`}
      </div>
    </>
  );
}

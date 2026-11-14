// The agent's task list (its `todo` tool), pinned above the composer: a
// fold of the chat's log (the latest `todo_updated`), so a reload, a second
// tab and a node restart show the same list. It is open while a turn works
// on it and collapsed to its head otherwise (every item done, or nothing
// running); opening or closing it is this tab's only state, until that
// changes. Every item is the model's text: shown as text, hidden
// characters marked. Its own lazy chunk: the conversation loads it on the
// chat's first list.
import { Check, ChevronDown, ChevronRight, Circle, CircleDot } from "lucide-react";
import { useEffect, useState } from "react";
import { showHidden } from "../md/hiddenChars";
import type { TodoItem } from "../types";
import "./todoPanel.css";

const MARK = {
  done: <Check size={14} aria-hidden />,
  in_progress: <CircleDot size={14} aria-hidden />,
  pending: <Circle size={14} aria-hidden />,
} as const;

const WORD = { done: "done", in_progress: "in progress", pending: "to do" } as const;

export default function TodoPanel({ items, running }: { items: TodoItem[]; running: boolean }) {
  const done = items.filter((i) => i.status === "done").length;
  const now = items.find((i) => i.status === "in_progress");
  const settled = done === items.length || !running;
  // The person's own open or close, until the list settles or wakes up.
  const [choice, setChoice] = useState<boolean | null>(null);
  useEffect(() => setChoice(null), [settled]);
  if (items.length === 0) return null;
  const open = choice ?? !settled;
  return (
    <section className="todo-panel" aria-label="The agent's task list">
      <button type="button" className="todo-head" aria-expanded={open} onClick={() => setChoice(!open)}>
        {open ? <ChevronDown size={14} aria-hidden /> : <ChevronRight size={14} aria-hidden />}
        <span className="todo-title">Tasks</span>
        <span className="todo-count">
          {done} of {items.length} done
        </span>
        {now && <span className="todo-now ellipsis">{showHidden(now.content)}</span>}
      </button>
      {open && (
        <ol className="todo-rows">
          {items.map((it, i) => (
            <li
              key={i}
              className={`todo-row todo-${it.status}`}
              data-status={it.status}
              aria-current={it.status === "in_progress" ? "step" : undefined}
              title={WORD[it.status]}
            >
              {MARK[it.status]}
              <span className="todo-text">{showHidden(it.content)}</span>
            </li>
          ))}
        </ol>
      )}
    </section>
  );
}

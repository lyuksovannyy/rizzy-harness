// The agent's task list (its `todo` tool), pinned above the composer: a
// fold of the chat's log (the latest `todo_updated`), so a reload, a second
// tab and a node restart show the same list. Collapsing it is this tab's
// only state. Every item is the model's text: shown as text, hidden
// characters marked. Its own lazy chunk: the conversation loads it on the
// chat's first list.
import { Check, ChevronDown, ChevronRight, Circle, CircleDot } from "lucide-react";
import { useState } from "react";
import { showHidden } from "../md/hiddenChars";
import type { TodoItem } from "../types";
import "./todoPanel.css";

const MARK = {
  done: <Check size={14} aria-hidden />,
  in_progress: <CircleDot size={14} aria-hidden />,
  pending: <Circle size={14} aria-hidden />,
} as const;

const WORD = { done: "done", in_progress: "in progress", pending: "to do" } as const;

export default function TodoPanel({ items }: { items: TodoItem[] }) {
  const [open, setOpen] = useState(true);
  if (items.length === 0) return null;
  const done = items.filter((i) => i.status === "done").length;
  const now = items.find((i) => i.status === "in_progress");
  return (
    <section className="todo-panel" aria-label="The agent's task list">
      <button type="button" className="todo-head" aria-expanded={open} onClick={() => setOpen((o) => !o)}>
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

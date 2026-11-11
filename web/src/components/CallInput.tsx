// "What this call will do": every key and value of a tool call's input, as
// it will run (lib/callText.ts has the rules). Used by every tool card, the
// approval above all.
import { useCallback, useState } from "react";
import {
  cutText,
  fmt,
  hiddenCount,
  marks,
  mixedScripts,
  pathNotes,
  sizeOf,
  sizeText,
  type Field,
  type Mode,
  type Seg,
} from "../lib/callText";
import { shownName } from "../md/hidden";

/** Which long fields the person expanded; one set per card. */
export function useOpenFields(): [ReadonlySet<string>, (key: string) => void] {
  const [open, setOpen] = useState<ReadonlySet<string>>(() => new Set());
  const toggle = useCallback(
    (key: string) =>
      setOpen((o) => {
        const n = new Set(o);
        if (!n.delete(key)) n.add(key);
        return n;
      }),
    [],
  );
  return [open, toggle];
}

function Marked({ text, mode }: { text: string; mode: Mode }) {
  return <>{marks(text, mode).map((s: Seg, i) => (s.mark ? <span key={i} className="hidden-mark">{s.t}</span> : s.t))}</>;
}

function Notes({ notes }: { notes: string[] }) {
  if (!notes.length) return null;
  return (
    <ul className="call-notes">
      {notes.map((n) => (
        <li key={n}>{n}</li>
      ))}
    </ul>
  );
}

/** What a person should know about a text beyond seeing it. */
function textNotes(value: string, mode: Mode): string[] {
  const notes: string[] = [];
  const hidden = hiddenCount(value);
  if (hidden)
    notes.push(
      `${fmt(hidden)} hidden, control or unusual blank character${hidden === 1 ? " is" : "s are"} shown as <U+…> codes.`,
    );
  // Look-alikes matter where a word is a name the machine acts on: a
  // command, a path, a short value. Prose and file content are left alone.
  if (mode === "cmd")
    for (const m of mixedScripts(value))
      notes.push(`“${shownName(m.word)}” mixes ${m.scripts.join(" and ")} letters: ${m.odd.join(", ")}.`);
  return notes;
}

const indent = (depth: number) => (depth ? { paddingLeft: `${Math.min(depth, 6) * 14}px` } : undefined);

function TextField({ f, open, onToggle }: { f: Extract<Field, { kind: "text" }>; open: boolean; onToggle: (k: string) => void }) {
  const size = sizeOf(f.value);
  const c = f.cut;
  const showSize = !!c || f.mode === "text" || size.lines > 1;
  const more = `Show all ${fmt(size.chars)} characters`;
  return (
    <div className="call-field" style={indent(f.depth)}>
      <div className="call-label">
        <span className="call-key">{shownName(f.label)}</span>
        {showSize && <span className="call-size">{sizeText(size)}</span>}
        {c && (
          <button type="button" className="link call-toggle" aria-expanded={open} onClick={() => onToggle(f.key)}>
            {open ? "Show less" : more}
          </button>
        )}
      </div>
      <pre className={`term call-text${f.tone ? ` call-${f.tone}` : ""}`} dir="ltr">
        {f.value === "" ? (
          <span className="hidden-mark">⟨empty⟩</span>
        ) : !c || open ? (
          <Marked text={f.value} mode={f.mode} />
        ) : (
          <>
            <Marked text={c.head} mode={f.mode} />
            <span className="call-cut" role="note">
              <span className="call-cut-text">⟨{cutText(c)}⟩</span>{" "}
              <button type="button" className="link" onClick={() => onToggle(f.key)}>
                {more}
              </button>
            </span>
            <Marked text={c.tail} mode={f.mode} />
          </>
        )}
      </pre>
      <Notes notes={textNotes(f.value, f.mode)} />
    </div>
  );
}

function FieldRow({ f, cwd, open, onToggle }: { f: Field; cwd?: string; open: ReadonlySet<string>; onToggle: (k: string) => void }) {
  switch (f.kind) {
    case "text":
      return <TextField f={f} open={open.has(f.key)} onToggle={onToggle} />;
    case "path":
      // Never cut: where a write goes is the first thing to check.
      return (
        <div className="call-field" style={indent(f.depth)}>
          <div className="call-label">
            <span className="call-key">{f.label}</span>
          </div>
          <pre className="term call-text call-path" dir="ltr">
            {f.value === "" ? <span className="hidden-mark">⟨empty⟩</span> : <Marked text={f.value} mode="cmd" />}
          </pre>
          <Notes notes={[...pathNotes(f.value, cwd), ...textNotes(f.value, "cmd")]} />
        </div>
      );
    case "value":
      return (
        <div className="call-field call-inline" style={indent(f.depth)}>
          <span className="call-key">{shownName(f.label)}</span> <code className="call-value">{f.value}</code>{" "}
          <span className="call-size">({f.type})</span>
        </div>
      );
    case "group":
      return (
        <div className="call-field call-inline" style={indent(f.depth)}>
          <span className="call-key">{shownName(f.label)}</span> <span className="call-size">{f.summary}</span>
        </div>
      );
  }
}

export function CallInput({
  fields,
  cwd,
  open,
  onToggle,
}: {
  fields: Field[];
  cwd?: string;
  open: ReadonlySet<string>;
  onToggle: (key: string) => void;
}) {
  if (!fields.length) return null;
  return (
    <div className="call-input">
      {fields.map((f) => (
        <FieldRow key={f.key} f={f} cwd={cwd} open={open} onToggle={onToggle} />
      ))}
    </div>
  );
}

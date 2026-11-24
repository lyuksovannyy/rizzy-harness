// A text file of the chat's folder beside the conversation (X.63): the way
// Claude Desktop and Codex open a file in a side panel, read-only. The node
// read the file now (at most 256 KiB, scrubbed of its secrets); this draws it
// as text with line numbers.
//
// Everything is the file's text, drawn as React text nodes: no HTML, no
// image, no link made from it. Hidden, control and direction characters are
// shown as codes (showHidden) before anything else is done with the text,
// highlighting included, so what is read is what is there. Highlighting is
// the chat's own (md/highlighter: a worker with a time budget, refused above
// the size the chat's code blocks accept); a file it will not do stays plain.
// Its own lazy chunk, loaded on the first "Open".
import { PanelRightClose, RefreshCw } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { call } from "../api";
import { bytes } from "../lib/bytes";
import type { PreviewTarget } from "../lib/docPreview";
import { type FileAnswer, fileUrl, langOf } from "../lib/folderFile";
import { type HastNode, renderHast } from "../md/hast";
import { showHidden } from "../md/hidden";
import { highlightOffThread } from "../md/highlighter";
import { grammarFor } from "../md/langs";
import { highlightable } from "../md/Code";
import "./docPreview.css";

/** Lines of text as the gutter numbers them: a last empty piece (the final newline) is no line. */
const lineCount = (t: string) => (t === "" ? 0 : t.split("\n").length - (t.endsWith("\n") ? 1 : 0));

export default function FolderFile(props: { target: PreviewTarget; onClose: () => void }) {
  const { session, name, path = "" } = props.target;
  const [data, setData] = useState<{ a: FileAnswer; at: Date } | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState(true);
  const [tries, setTries] = useState(0);
  const [tree, setTree] = useState<HastNode | null>(null);
  const head = useRef<HTMLHeadingElement>(null);

  useEffect(() => {
    const ac = new AbortController();
    setBusy(true);
    setErr(null);
    call<FileAnswer>("GET", fileUrl(session, path), undefined, ac.signal)
      .then((a) => {
        if (typeof a.text !== "string" || typeof a.size !== "number") throw new Error("the server's answer was not understood");
        setData({ a, at: new Date() });
        setBusy(false);
      })
      .catch((x: unknown) => {
        if (ac.signal.aborted) return;
        // The file as it was is not "now": say what is wrong, show nothing old.
        setData(null);
        setErr(x instanceof Error ? x.message : String(x));
        setBusy(false);
      });
    return () => ac.abort();
  }, [session, path, tries]);

  // Focus moves into the panel when it opens; Escape inside it closes it.
  useEffect(() => {
    head.current?.focus();
  }, []);

  const text = useMemo(() => (data ? showHidden(data.a.text) : ""), [data]);
  const lines = useMemo(() => lineCount(text), [text]);
  const gutter = useMemo(() => Array.from({ length: lines }, (_, i) => i + 1).join("\n"), [lines]);

  const grammar = grammarFor(langOf(name));
  useEffect(() => {
    setTree(null);
    if (!grammar || !text || !highlightable(text)) return;
    const gone = new AbortController();
    // A worker that cannot start leaves the text plain.
    highlightOffThread(text, grammar, gone.signal)
      .then((t) => {
        if (t && !gone.signal.aborted) setTree(t);
      })
      .catch(() => {});
    return () => gone.abort();
  }, [text, grammar]);

  const a = data?.a;
  return (
    <aside
      className="doc-preview file-view"
      aria-label={`File ${showHidden(name)}`}
      onKeyDown={(e) => {
        if (e.key === "Escape") props.onClose();
      }}
    >
      <div className="pv-head">
        <h2 className="pv-title small" tabIndex={-1} ref={head}>
          {showHidden(name)}
        </h2>
        <button type="button" className="icon small" aria-label="Reload the file" title="Read the file again" disabled={busy} onClick={() => setTries((n) => n + 1)}>
          <RefreshCw size={16} aria-hidden />
        </button>
        <button type="button" className="icon small" aria-label="Close the file" title="Close (Esc)" onClick={props.onClose}>
          <PanelRightClose size={16} aria-hidden />
        </button>
      </div>
      <p className="pv-note small muted mono fv-path">{showHidden(path)}</p>
      {a && data && (
        <p className="pv-note small muted">
          {bytes(a.size)} · {lines.toLocaleString("en-US")} line{lines === 1 ? "" : "s"} shown · as of {data.at.toLocaleTimeString()}. Read-only.
        </p>
      )}
      <div className="pv-body fv-body">
        {busy && (
          <p className="muted small" role="status">
            Reading the file…
          </p>
        )}
        {err && (
          <div className="error-text small" role="alert">
            {showHidden(err)}{" "}
            <button type="button" className="link" onClick={() => setTries((n) => n + 1)}>
              Try again
            </button>
          </div>
        )}
        {a && !busy && (
          <>
            {a.truncated && (
              <p className="pv-more small" role="note">
                Only the first {bytes(new TextEncoder().encode(a.text).length)} of this {bytes(a.size)} file is shown; the rest is not shown.
              </p>
            )}
            {a.lossy && (
              <p className="muted small" role="note">
                Some bytes are not valid text; they are shown as �.
              </p>
            )}
            {a.text === "" ? (
              <p className="muted small">The file is empty.</p>
            ) : (
              <div className="fv-code" role="region" aria-label={`Text of ${showHidden(name)}`} tabIndex={0}>
                <pre className="fv-gutter" aria-hidden="true">
                  {gutter}
                </pre>
                <pre className="fv-text">
                  <code>{tree ? renderHast(tree) : text}</code>
                </pre>
              </div>
            )}
          </>
        )}
      </div>
    </aside>
  );
}

// A background command's output in its row of the side panel (X.62): closed
// by default; open, it shows the tail the node holds (read-only, monospace),
// looked at again every few seconds while the command runs and once when it
// ended, never while closed, in a hidden tab or while the node is away.
// Like Claude Desktop's background task output, on demand; the text is the
// node's scrubbed and escape-free tail, shown as text in a <pre> with every
// hidden or control character made visible, never as HTML or a link.
import { type ReactNode, useEffect, useRef, useState } from "react";
import { HttpError } from "../api";
import { type BgOutput, POLL_MS, bytesWords, ranWords, readBgOutput } from "../lib/bgOutput";
import { showHidden } from "../md/hidden";

type Look = { out: BgOutput; error?: undefined } | { out?: undefined; error: string } | null;

export default function CommandOutput(props: {
  sessionId: string;
  bg: string;
  /** The log shows the command ended. */
  ended: boolean;
  /** The chat's node is known to be away. */
  away: boolean;
  /** The row's state in words, and its Stop: the foot row this sits in. */
  status: ReactNode;
  stop: ReactNode;
}) {
  const { sessionId, bg, ended, away } = props;
  const [open, setOpen] = useState(false);
  const [look, setLook] = useState<Look>(null);
  const box = useRef<HTMLPreElement>(null);
  const pinned = useRef(true);
  // Whether the node's last answer said it is over for good (forgotten, a
  // node that cannot): no more looks until the row is closed and opened.
  const settled = useRef(false);

  useEffect(() => {
    if (!open || away) return;
    settled.current = false;
    const ac = new AbortController();
    let timer: ReturnType<typeof setTimeout> | undefined;
    let last: BgOutput | undefined;
    const once = async () => {
      try {
        const got = await readBgOutput(sessionId, bg, ac.signal);
        // An answer that is not an object (an empty body) is no answer.
        if (!got || typeof got !== "object") throw new Error("The server sent no output.");
        last = got;
        setLook({ out: got });
      } catch (x) {
        if (ac.signal.aborted) return;
        // A 409 is the node's own word (forgotten, older): looking again says it again.
        settled.current = x instanceof HttpError && x.status !== 503 && x.status !== 504;
        setLook({ error: x instanceof Error ? x.message : String(x) });
      }
    };
    const next = () => {
      // Once more after the end shows in the log, then no more.
      const over = settled.current || (ended && last !== undefined && !last.running);
      if (over || ac.signal.aborted) return;
      timer = setTimeout(tick, POLL_MS);
    };
    const tick = async () => {
      if (document.visibilityState === "hidden") return next();
      await once();
      next();
    };
    // Back to a visible tab: look at once.
    const visible = () => {
      if (document.visibilityState === "visible" && !settled.current && !ac.signal.aborted) {
        clearTimeout(timer);
        void tick();
      }
    };
    document.addEventListener("visibilitychange", visible);
    void tick();
    return () => {
      ac.abort();
      clearTimeout(timer);
      document.removeEventListener("visibilitychange", visible);
    };
  }, [open, away, ended, sessionId, bg]);

  // Follow the end while the person has not scrolled up.
  const text = look?.out?.text;
  useEffect(() => {
    const el = box.current;
    if (el && pinned.current) el.scrollTop = el.scrollHeight;
  }, [text]);

  const out = look?.out;
  const state = out ? (out.running ? `running for ${ranWords(out.ran_ms)}` : `ran ${ranWords(out.ran_ms)}`) : null;
  return (
    <>
      <div className="side-row-foot">
        {props.status}
        <span className="side-row-actions">
          <button
            type="button"
            className="small"
            aria-expanded={open}
            aria-label={`${open ? "Hide" : "Show"} the output of ${bg}`}
            onClick={() => {
              setOpen(!open);
              setLook(null);
              pinned.current = true;
            }}
          >
            {open ? "Hide output" : "Output"}
          </button>
          {props.stop}
        </span>
      </div>
      {open && (
        <div className="side-output" data-bg={bg}>
          {away && <div className="small muted">The node is away, so its output cannot be read now.</div>}
          {!away && look === null && <div className="small muted">Reading…</div>}
          {look?.error !== undefined && (
            <div className="small error-text" role="status">
              {showHidden(look.error)}
            </div>
          )}
          {out && (
            <>
              <div className="small muted" role="status">
                {state} · {bytesWords(out.printed)} printed
              </div>
              {out.dropped ? (
                <div className="small muted">The node no longer keeps this output.</div>
              ) : (
                <>
                  {out.cut && <div className="small muted">… earlier output not shown</div>}
                  <pre
                    ref={box}
                    className="side-output-text mono"
                    tabIndex={0}
                    aria-label={`Output of ${bg}`}
                    onScroll={(e) => {
                      const el = e.currentTarget;
                      pinned.current = el.scrollHeight - el.scrollTop - el.clientHeight < 24;
                    }}
                  >
                    {out.text === "" ? "(nothing printed yet)" : showHidden(out.text)}
                  </pre>
                </>
              )}
            </>
          )}
        </div>
      )}
    </>
  );
}

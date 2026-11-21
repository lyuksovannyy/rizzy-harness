// "Fork from here" where the node can give the fork a folder of its own
// (ADOPTION_PLAN C.2 part 2, README "Fork a chat"): the link opens a small
// inline choice, no dialog, modelled on the rewind choice (RewindMessage):
// Escape or a click outside closes it, and the buttons answer nothing for a
// moment after it opens so a double click or the key that opened it does
// not choose. While the node copies the repository (it can take a while)
// both choices are off and the panel says so; it cannot be closed then,
// because the new chat opens when the copy is done. The node's refusal
// (not a git repository, no projects folder, a pruned checkpoint, no room)
// is shown in the panel, and the other choice stays there.
import { useEffect, useRef, useState } from "react";
import { pageApi } from "../apiPages";
import { errText } from "../lib/errorText";
import { raise } from "../lib/choiceGuard";
import { routeHash } from "../lib/routes";
import { GUARD_MS } from "./ApprovalChoice";
import "./forkChoice.css";

type Checkout = "own" | "shared";

export function ForkChoice(props: { sessionId: string; turnId: string }) {
  const [open, setOpen] = useState(false);
  const [armed, setArmed] = useState(false);
  const [busy, setBusy] = useState<Checkout | null>(null);
  const [err, setErr] = useState<string | null>(null);
  // The own copy was refused: the other choice is the likely one now.
  const [ownRefused, setOwnRefused] = useState(false);
  const root = useRef<HTMLDivElement>(null);
  const opener = useRef<HTMLButtonElement>(null);
  const panel = useRef<HTMLDivElement>(null);
  const alive = useRef(true);
  const working = useRef(false);
  useEffect(() => () => void (alive.current = false), []);

  const close = (refocus: boolean) => {
    if (working.current) return;
    setOpen(false);
    setErr(null);
    if (refocus) opener.current?.focus();
  };
  useEffect(() => {
    if (!open) return;
    setArmed(false);
    panel.current?.focus();
    const id = setTimeout(() => setArmed(true), GUARD_MS);
    const drop = raise();
    const away = (e: Event) => {
      if (e.target instanceof Node && !root.current?.contains(e.target)) close(false);
    };
    // Escape closes the panel wherever the focus went (a Tab can take it
    // out of the panel while it stays open).
    const esc = (e: KeyboardEvent) => {
      if (e.key === "Escape" && !e.defaultPrevented && !root.current?.contains(e.target as Node)) close(false);
    };
    document.addEventListener("pointerdown", away, true);
    document.addEventListener("keydown", esc);
    return () => {
      clearTimeout(id);
      drop();
      document.removeEventListener("pointerdown", away, true);
      document.removeEventListener("keydown", esc);
    };
  }, [open]);

  const run = async (checkout: Checkout) => {
    if (!armed || working.current) return;
    working.current = true;
    setBusy(checkout);
    setErr(null);
    try {
      const { session_id } = await pageApi.forkChat(props.sessionId, props.turnId, checkout);
      location.hash = routeHash({ kind: "session", id: session_id });
    } catch (x) {
      if (alive.current) {
        setErr(errText(x));
        if (checkout === "own") setOwnRefused(true);
      }
    } finally {
      working.current = false;
      if (alive.current) setBusy(null);
    }
  };
  const gate = armed ? undefined : true;
  const id = `fork-${props.turnId}`;
  return (
    <div
      className={`fork-line small${open || err ? " shown" : ""}`}
      ref={root}
      onKeyDown={(e) => {
        if (e.key === "Escape" && open) {
          e.stopPropagation();
          close(true);
        }
      }}
    >
      <button
        type="button"
        ref={opener}
        className="link"
        aria-expanded={open}
        aria-controls={open ? id : undefined}
        title="Start a new chat with this conversation up to here. This chat stays as it is."
        onClick={() => (open ? close(true) : setOpen(true))}
      >
        Fork from here
      </button>
      {open && (
        <div ref={panel} id={id} className="fork-panel" role="group" aria-label="Fork this chat" tabIndex={-1}>
          <p>Where should the new chat's files be?</p>
          <div className={`fork-choices${armed ? "" : " guarded"}`}>
            <button type="button" className={ownRefused ? "" : "primary"} disabled={busy !== null} aria-disabled={gate} aria-describedby={`${id}-own`} onClick={() => void run("own")}>
              In its own copy of the repository
            </button>
            <p id={`${id}-own`} className="small muted">
              A clone of this repository in this node's projects folder, with the files as they were here (uncommitted changes included). Files git ignores, such as build output and <span className="mono">.env</span>, are not copied.
            </p>
            <button type="button" className={ownRefused ? "primary" : ""} disabled={busy !== null} aria-disabled={gate} aria-describedby={`${id}-shared`} onClick={() => void run("shared")}>
              In the same folder
            </button>
            <p id={`${id}-shared`} className="small muted">
              The two chats share the files: what either one does there is in the other's folder.
            </p>
          </div>
          {busy === "own" && (
            <p className="small" role="status">
              Making the copy… the new chat opens when it is done.
            </p>
          )}
          {err && (
            <div className="error-text" role="alert">
              {err}
            </div>
          )}
        </div>
      )}
      {!open && err && <div className="error-text">{err}</div>}
    </div>
  );
}
